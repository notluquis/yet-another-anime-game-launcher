import { join } from "path-browserify";
import { build, CommandSegments, rawString } from "./command-builder";

// Neutralino 2026 standard with pragmatic fallback for dev mode
// Initialized synchronously before any resolve() calls
let _basePath: string = "/";

// Initialize synchronously using environment or fallback (called at module load time)
function initializeBasePathSync(): string {
  // NL_PATH is the --path argument passed to the binary (e.g. yaaglwdos).
  // NL_CWD is where `neu run` was executed (project root in dev).
  // We want NL_PATH as the app's working directory, resolved against NL_CWD if relative.
  try {
    const nlPath = (globalThis as any).NL_PATH;
    if (nlPath) {
      if (nlPath.startsWith("/")) {
        _basePath = nlPath;
        return _basePath;
      }
      // NL_PATH is relative — resolve against NL_CWD
      const nlCwd = (globalThis as any).NL_CWD;
      if (nlCwd && nlCwd.startsWith("/")) {
        _basePath = join(nlCwd, nlPath);
        return _basePath;
      }
    }
  } catch (e) {
    // continue
  }

  // Fallback: use NL_CWD directly (production .app sets NL_PATH as absolute)
  try {
    const nlCwd = (globalThis as any).NL_CWD;
    if (nlCwd && nlCwd.startsWith("/")) {
      _basePath = nlCwd;
      return _basePath;
    }
  } catch (e) {
    // continue
  }

  return _basePath;
}

// Initialize base path at module load time (synchronously)
initializeBasePathSync();

export function resolve(path: string): string {
  if (path.startsWith("/")) {
    return path;
  }

  // Ensure _basePath is absolute before using it
  let basePath = _basePath;
  if (!basePath.startsWith("/")) {
    // If basePath is relative, this likely means we're in dev mode
    // For now throw error - should be fixed by async initializeBasePath()
    // But we can give more useful context
    throw new Error(
      `resolve() called before basePath initialized. basePath="${basePath}". Call await initializeBasePath() early in app initialization.`
    );
  }

  const resolved = join(basePath, path);
  // Final validation
  if (!resolved.startsWith("/") || resolved === "/") {
    throw new Error("Assertation failed " + resolved);
  }
  return resolved;
}

// Async initialization to get the real pwd-based path if NL_PATH didn't work
let _initPromise: Promise<void> | null = null;

export async function initializeBasePath(): Promise<void> {
  if (_initPromise) return _initPromise;

  _initPromise = (async () => {
    // If we already have a good path from NL_CWD or other means, we're done
    if (_basePath && _basePath !== "/") {
      return;
    }

    // Try to get pwd from multiple sources (fastest to slowest)
    try {
      // 1. Try environment variable (fastest, doesn't need process execution)
      if (typeof process !== "undefined" && process.env?.PWD?.startsWith("/")) {
        _basePath = process.env.PWD;
        return;
      }
    } catch (e) {
      // continue
    }

    // 2. Fall back to exec pwd
    try {
      const result = await exec(["pwd"]);
      _basePath = result.stdOut.trim();
      if (!_basePath.startsWith("/")) {
        throw new Error(`pwd returned non-absolute path: "${_basePath}"`);
      }
      return;
    } catch (e) {
      console.error("Failed to initialize basePath with pwd:", e);
    }

    // 3. Last resort: try to extract from NL_CWD (in case it wasn't available sync)
    try {
      const nlCwd = (globalThis as any).NL_CWD;
      if (nlCwd && nlCwd.startsWith("/")) {
        _basePath = nlCwd;
      }
    } catch {
      // Give up
    }
  })();

  return _initPromise;
}

export async function exec(
  segments: CommandSegments,
  env?: { [key: string]: string },
  sudo = false,
  log_redirect: string | undefined = undefined
): Promise<Neutralino.os.ExecCommandResult> {
  const cmd = build(
    [...segments, ...(log_redirect ? [rawString("&>"), log_redirect] : [])],
    env
  );
  await log(sudo ? runInSudo(cmd) : cmd);
  const ret = await Neutralino.os.execCommand(sudo ? runInSudo(cmd) : cmd, {});
  if (ret.exitCode != 0) {
    throw new Error(
      `Command return non-zero code (${ret.exitCode}) \n${cmd}\nStdOut:\n${ret.stdOut}\nStdErr:\n${ret.stdErr}`
    );
  }
  return ret;
}

export async function exec2(
  segments: CommandSegments,
  env?: { [key: string]: string },
  sudo = false,
  log_redirect: string | undefined = undefined
): Promise<Neutralino.os.ExecCommandResult> {
  const cmd = build(
    [...segments, ...(log_redirect ? [rawString("&>"), log_redirect] : [])],
    env
  );
  await log(cmd);
  const { id, pid } = await Neutralino.os.spawnProcess(cmd);
  return await new Promise((res, rej) => {
    const handler: Neutralino.events.Handler<
      Neutralino.os.SpawnProcessResult
    > = event => {
      if (!event) return;
      let stdErr = "",
        stdOut = "";
      if (event.detail.id == id) {
        if (event.detail["action"] == "exit") {
          const exit = Number(event.detail["data"]);
          if (exit == 0) {
            res({
              pid,
              exitCode: exit,
              stdErr,
              stdOut,
            });
          } else {
            rej(
              new Error(
                `Command return non-zero code (${exit}) \n${cmd}\nStdOut:\n${stdOut}\nStdErr:\n${stdErr}`
              )
            );
          }

          Neutralino.events.off("spawnedProcess", handler);
        } else if (event.detail["action"] == "stdOut") {
          stdOut += event.detail["data"];
        } else if (event.detail["action"] == "stdErr") {
          stdErr += event.detail["data"];
        }
      }
    };
    Neutralino.events.on("spawnedProcess", handler);
  });
}

export function runInSudo(cmd: string) {
  return build([
    "osascript",
    "-e",
    [
      "do",
      "shell",
      "script",
      `"${`${cmd}`.replaceAll("\\", "\\\\").replaceAll('"', '\\"')}"`,
      "with",
      "administrator",
      "privileges",
    ].join(" "),
  ]);
}

export function tar_extract(src: string, dst: string) {
  return exec(["tar", "-zxvf", src, "-C", dst]);
}

export function tar_extract_directory(
  src: string,
  dst: string,
  dir: string,
  isXZ: boolean
) {
  const stripCount = dir.split("/").length;
  return exec([
    "tar",
    `--strip-components=${stripCount}`,
    "-C",
    dst,
    isXZ ? "-Jxvf" : "-zxvf",
    src,
    dir,
  ]);
}

export async function spawn(
  segments: CommandSegments,
  env?: { [key: string]: string }
) {
  const cmd = build(segments, env);
  await log(cmd);
  const { pid, id } = await Neutralino.os.spawnProcess(cmd);
  // await Neutralino.os.
  await log(pid + "");
  await log(cmd);
  return { pid, id };
}

export async function getKey(key: string): Promise<string> {
  return await Neutralino.storage.getData(key);
}

export async function getKeyOrDefault(
  key: string,
  defaultValue: string
): Promise<string> {
  try {
    return await getKey(key);
  } catch {
    return defaultValue;
  }
}

export async function setKey(key: string, value: string | null) {
  return await Neutralino.storage.setData(key, value);
}

export function log(message: string) {
  return Neutralino.debug.log(message, "INFO");
}

export function warn(message: string) {
  return Neutralino.debug.log(message, "WARNING");
}

export function logerror(message: string) {
  return Neutralino.debug.log(message, "ERROR");
}

export function restart() {
  return Neutralino.app.restartProcess();
}

export async function fatal(error: unknown) {
  await Neutralino.os.showMessageBox(
    "Fatal error",
    `${error instanceof Error ? String(error) : JSON.stringify(error)}`,
    "OK"
  );
  await shutdown();
  Neutralino.app.exit(-1);
}

export async function appendFile(path: string, content: string) {
  await Neutralino.filesystem.appendFile(resolve(path), content);
}

export async function forceMove(source: string, destination: string) {
  return await exec([
    "mv",
    "-f",
    `${resolve(source)}`,
    `${resolve(destination)}`,
  ]);
}

export async function cp(source: string, destination: string) {
  return await exec([
    "cp",
    "-p",
    `${resolve(source)}`,
    `${resolve(destination)}`,
  ]);
}

export async function rmrf_dangerously(target: string) {
  return await exec(["rm", "-rf", resolve(target)]);
}

export async function prompt(title: string, message: string) {
  const out = await Neutralino.os.showMessageBox(title, message, "YES_NO");
  return out == "YES";
}

export async function alert(title: string, message: string) {
  return await Neutralino.os.showMessageBox(title, message, "OK");
}

export async function openDir(title: string) {
  const out = await Neutralino.os.showFolderDialog(title, {});
  return out;
}

export async function readFile(path: string) {
  return await Neutralino.filesystem.readFile(resolve(path));
}

export async function readBinary(path: string) {
  return await Neutralino.filesystem.readBinaryFile(resolve(path));
}

export async function readAllLines(path: string) {
  const content = await Neutralino.filesystem.readFile(resolve(path));
  if (content.indexOf("\r\n") >= 0) {
    return content.split("\r\n");
  }
  return content.split("\n");
}

export async function readAllLinesIfExists(path: string) {
  try {
    await stats(resolve(path));
  } catch {
    return [];
  }
  const content = await Neutralino.filesystem.readFile(resolve(path));
  if (content.indexOf("\r\n") >= 0) {
    return content.split("\r\n");
  }
  return content.split("\n");
}

export async function writeBinary(path: string, data: ArrayBuffer) {
  return await Neutralino.filesystem.writeBinaryFile(resolve(path), data);
}

export async function writeFile(path: string, data: string) {
  return await Neutralino.filesystem.writeFile(resolve(path), data);
}

export async function removeFile(path: string) {
  return await Neutralino.filesystem.removeFile(resolve(path));
}

export async function removeFileIfExists(path: string) {
  try {
    await stats(resolve(path));
  } catch {
    return;
  }
  return await Neutralino.filesystem.removeFile(resolve(path));
}

export async function stats(path: string) {
  return await Neutralino.filesystem.getStats(resolve(path));
}

export async function fileOrDirExists(path: string) {
  try {
    await stats(path);
    return true;
  } catch {
    return false;
  }
}

export async function env(key: string) {
  return Neutralino.os.getEnv(key);
}

export function exit(exitCode: number) {
  return Neutralino.app.exit(exitCode);
}

export function getMemoryInfo() {
  return Neutralino.computer.getMemoryInfo();
}

export function getCPUInfo() {
  return Neutralino.computer.getCPUInfo();
}

export function open(url: string) {
  return Neutralino.os.open(url);
}

export const sha1sum = async (message: string) => {
  const encoder = new TextEncoder();
  const data = encoder.encode(message);
  const hashBuffer = await crypto.subtle.digest("SHA-1", data);
  const hashArray = Array.from(new Uint8Array(hashBuffer)); // convert buffer to byte array
  const hashHex = hashArray.map(b => b.toString(16).padStart(2, "0")).join(""); // convert bytes to hex string
  return hashHex;
};

const hooks: Array<(forced: boolean) => Promise<boolean>> = [];

export function addTerminationHook(fn: (forced: boolean) => Promise<boolean>) {
  hooks.push(fn);
  const len = hooks.length;
  return () => {
    if (hooks.length !== len) {
      throw new Error("Unexpected behavior!");
    }
    hooks.pop();
  };
}

// ??
export async function GLOBAL_onClose(forced: boolean) {
  for (const hook of hooks.reverse()) {
    if (!(await hook(forced)) && !forced) {
      return false; // aborted
    }
  }
  return true;
}

export async function shutdown() {
  for (const hook of hooks.reverse()) {
    await hook(true);
  }
}

export async function _safeRelaunch() {
  await shutdown();
  if (import.meta.env.PROD) {
    let appPath: string | undefined;

    try {
      // Method 1: Try to find the running .app from process (most reliable)
      const result = await exec([
        "lsof",
        "-p",
        "$$",
        "-a",
        "-d",
        "cwd",
        "-n",
        "-b",
      ]);
      // This will show current directory, use as fallback location
    } catch {
      // Continue to next method
    }

    try {
      // Method 2: Get from config globalVariables (fallback location)
      const config = await Neutralino.app.getConfig();
      if (config.globalVariables?.appPath) {
        appPath = config.globalVariables.appPath;
      } else if (config.globalVariables?.appName) {
        appPath = `/Applications/${config.globalVariables.appName}.app`;
      }
    } catch {
      // Continue to fallback
    }

    // Method 3: Use standard locations in order
    if (!appPath) {
      const standardPaths = [
        "/Applications/Yaagl OS.app",
        "/Applications/Yaagl.app",
        process.env.HOME + "/Applications/Yaagl OS.app",
      ];

      for (const path of standardPaths) {
        try {
          await stats(path);
          appPath = path;
          break;
        } catch {
          // Try next path
        }
      }
    }

    // Fallback: use app name via open -a
    if (!appPath) {
      appPath = "Yaagl OS";
    }

    try {
      await Neutralino.os.execCommand(`open "${appPath}"`, {
        background: true,
      });
    } catch {
      // If path open fails, try by name
      await Neutralino.os.execCommand(`open -a "Yaagl OS"`, {
        background: true,
      });
    }
  } else {
    Neutralino.app.restartProcess();
  }
  Neutralino.app.exit(0);
}
