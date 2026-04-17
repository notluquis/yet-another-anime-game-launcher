import { exec, log, resolve, spawn, writeFile } from "./neu";
import { mkdirp } from "./unix";
import type { Config } from "@config";

export type LoggingPreset = "off" | "basic" | "deep";

export interface LoggingSession {
  dir: string;
  pids: number[];
  preset: LoggingPreset;
  startedAt: number;
  gameLogPath?: string;
}

export interface LoggingContext {
  config?: Partial<Config>;
  wineBackend?: string;
  wineDistro?: string;
  gameLogPath?: string;
  dxmtVersion?: string;
}

export interface StartLoggingResult {
  session: LoggingSession;
  envOverrides: { [key: string]: string };
}

const LOG_PREDICATE = [
  `(subsystem == "com.apple.Metal")`,
  `(subsystem BEGINSWITH "com.apple.MTLCompiler")`,
  `(subsystem == "com.apple.coreanimation")`,
  `(subsystem == "com.apple.SkyLight")`,
  `(subsystem == "com.apple.WindowServer")`,
  `(processImagePath CONTAINS[c] "GenshinImpact")`,
  `(processImagePath CONTAINS[c] "YuanShen")`,
  `(senderImagePath CONTAINS "winemac.drv")`,
  `(senderImagePath CONTAINS "dxmt")`,
].join(" OR ");

// Wrap any string in single quotes for safe interpolation into a POSIX shell.
// The only char that can break a single-quoted string is another single quote.
function shq(s: string): string {
  return `'${s.replace(/'/g, `'\\''`)}'`;
}

function sessionStamp() {
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return (
    `${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}` +
    `-${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}`
  );
}

async function xctraceAvailable(): Promise<boolean> {
  try {
    await exec(["xcrun", "--find", "xctrace"], {}, false, "/dev/null");
    return true;
  } catch {
    return false;
  }
}

async function captureSysInfo(): Promise<string> {
  try {
    const ret = await exec([
      "sh",
      "-c",
      "sw_vers -productName; sw_vers -productVersion; sw_vers -buildVersion; " +
        "sysctl -n hw.model; sysctl -n machdep.cpu.brand_string; " +
        "sysctl -n hw.memsize; sysctl -n hw.ncpu; sysctl -n hw.perflevel0.physicalcpu; " +
        "sysctl -n hw.perflevel1.physicalcpu 2>/dev/null || echo 0; " +
        "df -h / | tail -1 | awk '{print $4}'",
    ]);
    const lines = ret.stdOut.trim().split("\n");
    const [
      product,
      version,
      build,
      model,
      cpu,
      mem,
      ncpu,
      pcores,
      ecores,
      freeDisk,
    ] = lines;
    const memGb = mem ? `${Math.round(Number(mem) / 1024 / 1024 / 1024)} GB` : "?";
    return [
      `- OS: ${product ?? "?"} ${version ?? "?"} (${build ?? "?"})`,
      `- Model: ${model ?? "?"}`,
      `- CPU: ${cpu ?? "?"} (${ncpu ?? "?"} cores: ${pcores ?? "?"}P + ${ecores ?? "?"}E)`,
      `- RAM: ${memGb}`,
      `- Free disk on /: ${freeDisk ?? "?"}`,
    ].join("\n");
  } catch {
    return `- (sysinfo capture failed)`;
  }
}

function formatConfigSnapshot(ctx: LoggingContext): string {
  const c = ctx.config ?? {};
  const entries: [string, unknown][] = [
    ["wineDistro", ctx.wineDistro ?? c.wineDistro],
    ["wineBackend", ctx.wineBackend],
    ["dxmtVersion", ctx.dxmtVersion],
    ["retina", c.retina],
    ["leftCmd", c.leftCmd],
    ["captureDisplaysForFullscreen", c.captureDisplaysForFullscreen],
    ["metalHud", c.metalHud],
    ["metalfxSpatialFactor", c.metalfxSpatialFactor],
    ["reshade", c.reshade],
    ["hk4eEnableHDR", c.hk4eEnableHDR],
    ["steamPatch", c.steamPatch],
    ["blockNet", c.blockNet],
    ["proxyEnabled", c.proxyEnabled],
    ["loggingSession", c.loggingSession],
  ];
  return entries
    .filter(([, v]) => v !== undefined)
    .map(([k, v]) => `- ${k}: ${String(v)}`)
    .join("\n");
}

async function gatherMeta(
  dir: string,
  preset: LoggingPreset,
  ctx: LoggingContext
) {
  const sysinfo = await captureSysInfo();
  const configSnapshot = formatConfigSnapshot(ctx);

  const lines: (string | false | undefined)[] = [
    `# Session ${dir}`,
    ``,
    `Preset: **${preset}**`,
    `Started: ${new Date().toISOString()}`,
    ``,
    `## System`,
    sysinfo,
    ``,
    `## Config snapshot`,
    configSnapshot || `- (no config provided)`,
    ``,
    `## Files`,
    `- system.ndjson — unified logging (Metal/MTLCompiler/coreanimation/SkyLight/Wine/Genshin), newline-delimited JSON`,
    `- thermal.log — \`pmset -g therm\` sampled every 2s (look for CPU_Speed_Limit < 100)`,
    preset === "deep" &&
      `- trace.trace — Xcode Instruments "Game Performance" trace (open with \`open trace.trace\`)`,
    ctx.gameLogPath &&
      `- game.log — symlink to WINEDEBUG output (\`${ctx.gameLogPath}\`)`,
    `- dxmt.log — symlink to DXMT per-session log (if DXMT emitted one)`,
    `- analysis.md — auto-generated post-stop summary`,
    `- logger.err / xctrace.log / thermal.err — stderr of the background recorders`,
    ``,
    `## Analysis cheat-sheet`,
    ``,
    `### Quick view`,
    "```",
    `cat analysis.md`,
    "```",
    ``,
    `### Metal pipeline compiles (PSO stalls)`,
    "```",
    `grep -c '"subsystem":"com.apple.MTLCompiler' system.ndjson`,
    `grep -E 'newRenderPipelineState|newComputePipelineState' system.ndjson | head`,
    "```",
    ``,
    `### DXMT drawable double-present bug`,
    "```",
    `grep 'should not be called after already presenting' system.ndjson`,
    "```",
    ``,
    `### Direct vs Composited / display reconfigure`,
    "```",
    `grep -E 'CGSDisplayWillReconfigure|kCGSDisplayDidReconfigure|captured' system.ndjson | head -20`,
    "```",
    ``,
    `### Thermal throttling`,
    "```",
    `awk '/CPU_Speed_Limit/ {print}' thermal.log | sort -u`,
    `# any CPU_Speed_Limit < 100 means throttled`,
    "```",
    ``,
    `### winemac.drv window constraint errors (popup/window churn)`,
    "```",
    `grep -c '_CGXPackagesSetWindowConstraints: Invalid window' system.ndjson`,
    "```",
    ``,
    `### Wine errors / SEH (from game.log)`,
    "```",
    `grep -iE '^err:|seh|exception' game.log | head`,
    `grep -oE 'fixme:[a-zA-Z_]+:[a-zA-Z_0-9]+' game.log | sort | uniq -c | sort -rn | head -20`,
    "```",
    ``,
    `### Processes observed`,
    "```",
    `grep -oE '"processImagePath":"[^"]*"' system.ndjson | sort -u`,
    "```",
  ];
  await writeFile(
    `${dir}/meta.md`,
    lines.filter(Boolean).join("\n")
  );
}

export async function startLoggingSession(
  preset: LoggingPreset,
  ctx: LoggingContext = {}
): Promise<StartLoggingResult | undefined> {
  if (preset === "off") return undefined;

  const stamp = sessionStamp();
  const dir = resolve(`./logs/sessions/${stamp}`);
  await mkdirp(dir);
  await log(`[logging-session] starting preset=${preset} dir=${dir}`);

  const effective: LoggingPreset =
    preset === "deep" && !(await xctraceAvailable()) ? "basic" : preset;
  if (effective !== preset) {
    await log(
      `[logging-session] xctrace not found — degrading deep -> basic`
    );
  }

  await gatherMeta(dir, effective, ctx);

  const pids: number[] = [];

  // Layer 1 — unified logging (always for basic+deep)
  const logScript = `${dir}/_logger.sh`;
  await writeFile(
    logScript,
    `#!/bin/sh
exec log stream --style ndjson --level debug \\
  --predicate ${shq(LOG_PREDICATE)} \\
  >> ${shq(`${dir}/system.ndjson`)} \\
  2>> ${shq(`${dir}/logger.err`)}
`
  );
  try {
    const { pid } = await spawn(["sh", logScript]);
    pids.push(pid);
  } catch (e) {
    await log(`[logging-session] failed to start log stream: ${e}`);
  }

  // Layer 2 — thermal sampler (cheap, no sudo)
  const thermScript = `${dir}/_thermal.sh`;
  await writeFile(
    thermScript,
    `#!/bin/sh
out=${shq(`${dir}/thermal.log`)}
err=${shq(`${dir}/thermal.err`)}
while :; do
  printf 'T=%s ' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >> "$out" 2>> "$err"
  pmset -g therm 2>> "$err" | tr '\\n' ' ' >> "$out" 2>> "$err"
  printf '\\n' >> "$out"
  sleep 2
done
`
  );
  try {
    const { pid } = await spawn(["sh", thermScript]);
    pids.push(pid);
  } catch (e) {
    await log(`[logging-session] failed to start thermal sampler: ${e}`);
  }

  // Layer 3 — xctrace (deep only)
  if (effective === "deep") {
    const xcScript = `${dir}/_xctrace.sh`;
    await writeFile(
      xcScript,
      `#!/bin/sh
exec xcrun xctrace record \\
  --template 'Game Performance' \\
  --all-processes \\
  --output ${shq(`${dir}/trace.trace`)} \\
  >> ${shq(`${dir}/xctrace.log`)} 2>&1
`
    );
    try {
      const { pid } = await spawn(["sh", xcScript]);
      pids.push(pid);
    } catch (e) {
      await log(`[logging-session] failed to start xctrace: ${e}`);
    }
  }

  // Env overrides for the game process — redirects DXMT's own log into the
  // session dir and bumps verbosity so we get useful warnings without flooding.
  const envOverrides: { [key: string]: string } = {
    DXMT_LOG_PATH: dir,
    DXMT_LOG_LEVEL: effective === "deep" ? "info" : "warn",
  };

  return {
    session: {
      dir,
      pids,
      preset: effective,
      startedAt: Date.now(),
      gameLogPath: ctx.gameLogPath,
    },
    envOverrides,
  };
}

async function runAnalyzer(session: LoggingSession) {
  const script = `${session.dir}/_analyze.sh`;
  const scriptBody = `#!/bin/sh
# Best-effort analyzer — no set -e so partial failures still produce output.
cd ${shq(session.dir)} || exit 0

SYS=system.ndjson
GAME=game.log
THERM=thermal.log

# grep -c always prints a number; exit code 1 when no match, so we must
# swallow it without letting '|| echo 0' duplicate the count.
gc() { grep -c "$1" "$2" 2>/dev/null; return 0; }
gcE() { grep -cE "$1" "$2" 2>/dev/null; return 0; }

{
  echo "# Auto analysis"
  echo
  echo "Generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo
  echo "## Unified log counts"
  if [ -f "$SYS" ]; then
    echo "- Events total: $(wc -l < "$SYS" | tr -d ' ')"
    echo "- Errors+Faults: $(gcE '"messageType":"(Error|Fault)"' "$SYS")"
    echo "- MTLCompiler events (PSO compiles): $(gc '"subsystem":"com.apple.MTLCompiler' "$SYS")"
    echo "- Drawable double-present bug hits: $(gc 'should not be called after already presenting' "$SYS")"
    echo "- Window constraint errors: $(gc '_CGXPackagesSetWindowConstraints: Invalid window' "$SYS")"
    echo "- CGSDisplayWillReconfigure: $(gc 'CGSDisplayWillReconfigure' "$SYS")"
    echo "- SkyLight datagram buffer clears: $(gc 'Clearing datagram buffer' "$SYS")"
  else
    echo "(no system.ndjson)"
  fi
  echo

  echo "## Error breakdown by subsystem"
  if [ -f "$SYS" ]; then
    echo '\`\`\`'
    grep -E '"messageType":"(Error|Fault)"' "$SYS" \\
      | grep -oE '"subsystem":"[^"]*"' \\
      | sort | uniq -c | sort -rn | head -15
    echo '\`\`\`'
  fi
  echo

  echo "## Thermal throttling"
  if [ -f "$THERM" ]; then
    minlimit=$(awk 'match($0, /CPU_Speed_Limit = [0-9]+/) {v=substr($0, RSTART+18, RLENGTH-18); if (min=="" || v<min) min=v} END {print (min==""?"?":min)}' "$THERM")
    minavail=$(awk 'match($0, /CPU_Available_CPUs = [0-9]+/) {v=substr($0, RSTART+21, RLENGTH-21); if (min=="" || v<min) min=v} END {print (min==""?"?":min)}' "$THERM")
    samples=$(wc -l < "$THERM" | tr -d ' ')
    echo "- Samples: $samples"
    echo "- Min CPU_Speed_Limit (100=no throttle): $minlimit"
    echo "- Min CPU_Available_CPUs: $minavail"
    echo
    if [ "$minlimit" != "100" ] && [ "$minlimit" != "?" ]; then
      echo "**⚠ Thermal throttling detected** — CPU was capped below 100% at some point."
    fi
  else
    echo "(no thermal.log)"
  fi
  echo

  echo "## Wine (game.log)"
  if [ -f "$GAME" ]; then
    totalfix=$(gc 'fixme:' "$GAME")
    totalerr=$(gcE '^[0-9a-f:.]* *err:' "$GAME")
    echo "- fixme total: $totalfix"
    echo "- err total: $totalerr"
    echo
    echo "### Top fixme: (module:function)"
    echo '\`\`\`'
    grep -oE 'fixme:[a-zA-Z_]+:[a-zA-Z_0-9]+' "$GAME" | sort | uniq -c | sort -rn | head -20
    echo '\`\`\`'
    echo
    echo "### Top err: lines (deduped)"
    echo '\`\`\`'
    grep -E '^[0-9a-f:.]* *err:' "$GAME" | sed 's/[0-9a-f]\\{4,\\}/#/g' | sort -u | head -20
    echo '\`\`\`'
  else
    echo "(no game.log symlink — maybe the game didn't start or the log was rotated)"
  fi
  echo

  echo "## Wine PID chain"
  if [ -f "$SYS" ]; then
    echo '\`\`\`'
    awk -F '"' '
      /x86_64-unix/ {
        for (i=1;i<=NF;i++) {
          if ($i=="processID") {pid=$(i+1); gsub(/[^0-9]/,"",pid)}
          if ($i=="timestamp") {ts=$(i+2)}
        }
        if (pid && ts && !seen[pid]) {seen[pid]=ts; print ts, "pid="pid}
      }
    ' "$SYS" | sort | head -30
    echo '\`\`\`'
  fi
  echo

  echo "## Unique processes in unified log"
  if [ -f "$SYS" ]; then
    echo '\`\`\`'
    grep -oE '"processImagePath":"[^"]*"' "$SYS" | sort -u | sed 's/"processImagePath":"//; s/"$//'
    echo '\`\`\`'
  fi
} > analysis.md
`;
  await writeFile(script, scriptBody);
  try {
    await exec(["sh", script]);
  } catch (e) {
    await log(`[logging-session] analyzer failed: ${e}`);
  }
}

export async function stopLoggingSession(session: LoggingSession) {
  const durationMs = Date.now() - session.startedAt;
  await log(
    `[logging-session] stopping ${session.dir} duration=${durationMs}ms pids=${session.pids.join(",")}`
  );

  // SIGINT first so xctrace flushes the trace properly.
  for (const pid of session.pids) {
    try {
      await exec(
        ["kill", "-INT", String(pid)],
        {},
        false,
        "/dev/null"
      );
    } catch {
      // process may have died already
    }
  }

  // Only xctrace needs time to serialize its .trace bundle.
  if (session.preset === "deep") {
    await new Promise(r => setTimeout(r, 2500));
  }

  // Then ensure nothing is left behind.
  for (const pid of session.pids) {
    try {
      await exec(["kill", String(pid)], {}, false, "/dev/null");
    } catch {
      // already gone
    }
  }

  // Link the WINEDEBUG game log into the session dir for convenience.
  if (session.gameLogPath) {
    try {
      await exec([
        "ln",
        "-sf",
        session.gameLogPath,
        `${session.dir}/game.log`,
      ]);
    } catch {
      // non-fatal
    }
  }

  // Link DXMT's latest log (DXMT writes dxmt-<timestamp>.log in DXMT_LOG_PATH).
  try {
    await exec([
      "sh",
      "-c",
      `ls -1t ${shq(`${session.dir}`)}/dxmt-*.log 2>/dev/null | head -1 | ` +
        `xargs -I{} ln -sf {} ${shq(`${session.dir}/dxmt.log`)}`,
    ]);
  } catch {
    // non-fatal
  }

  // Generate the post-stop analysis.md.
  await runAnalyzer(session);

  const summary = [
    `## Session complete`,
    ``,
    `Duration: ${(durationMs / 1000).toFixed(1)}s`,
    `Ended: ${new Date().toISOString()}`,
    ``,
    `See \`analysis.md\` for auto-generated summary.`,
    `Open the directory in Finder: \`open ${session.dir}\``,
  ].join("\n");
  try {
    await writeFile(`${session.dir}/README.md`, summary);
  } catch {
    // non-fatal
  }
}
