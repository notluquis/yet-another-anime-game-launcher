import { eq } from "semver";
import { Aria2 } from "@aria2";
import { CommonUpdateProgram } from "@common-update-ui";
import {
  mkdirp,
  resolve,
  humanFileSize,
  setKey,
  getKeyOrDefault,
  fileOrDirExists,
  doStreamUnzip,
  forceMove,
  readBinary,
  writeBinary,
  writeFile,
  rmrf_dangerously,
  exec,
  removeFile,
} from "@utils";
import { Wine } from "@wine";
import { join } from "path-browserify";

export const DXMT_FILES = ["d3d10core.dll", "d3d11.dll", "dxgi.dll"];

const DXMT_FILES_WITH_UNIXLIB = [
  ...DXMT_FILES,
  "winemetal.dll",
  "winemetal.so",
  "nvngx.dll",
];

const CURRENT_DXMT_VERSION = "0.74.0";

export async function* checkAndDownloadDXMT(aria2: Aria2): CommonUpdateProgram {
  if (
    eq(
      CURRENT_DXMT_VERSION,
      await getKeyOrDefault("installed_dxmt_version", "0.0.0")
    )
  ) {
    return;
  }

  await rmrf_dangerously(resolve(`./dxmt`));
  await mkdirp("./dxmt");
  yield ["setStateText", "DOWNLOADING_ENVIRONMENT"];
  const archiveName = "dxmt-v0.74-builtin-signed.tar.xz";
  for await (const progress of aria2.doStreamingDownload({
    uri: `https://github.com/dawn-winery/dawn-signed/releases/download/dxmt-v0.74-builtin-signed/${archiveName}`,
    absDst: resolve(`./dxmt/${archiveName}`),
  })) {
    yield [
      "setProgress",
      Number((progress.completedLength * BigInt(100)) / progress.totalLength),
    ];
    yield [
      "setStateText",
      "DOWNLOADING_ENVIRONMENT_SPEED",
      `${humanFileSize(Number(progress.downloadSpeed))}`,
    ];
  }

  yield ["setStateText", "EXTRACT_ENVIRONMENT"];
  yield ["setUndeterminedProgress"];
  await exec([
    "tar",
    "-xvf",
    resolve(`./dxmt/${archiveName}`),
    "-C",
    resolve("./dxmt"),
  ]);
  await exec([
    "sh",
    "-c",
    `mv "${resolve(
      "./dxmt/dxmt-v0.74-builtin-signed/x86_64-windows/"
    )}"* "${resolve("./dxmt/")}"`,
  ]);
  await exec([
    "sh",
    "-c",
    `mv "${resolve(
      "./dxmt/dxmt-v0.74-builtin-signed/x86_64-unix/"
    )}"* "${resolve("./dxmt/")}"`,
  ]);
  await rmrf_dangerously(resolve(`./dxmt/dxmt-v0.74-builtin-signed`));
  await removeFile(resolve(`./dxmt/${archiveName}`));
  await setKey("installed_dxmt_version", CURRENT_DXMT_VERSION);

  // Write default dxmt.conf only if it doesn't already exist (preserve user customizations)
  const dxmtConf = resolve("./dxmt.conf");
  if (!(await fileOrDirExists(dxmtConf))) {
    await writeFile(
      dxmtConf,
      [
        "# DXMT configuration — edit freely, this file is only created once.",
        "",
        "# MetalFX Spatial upscaling is disabled by default (factor 1.0).",
        "# Only useful on Retina displays where backing store is 2x logical.",
        "# On non-Retina monitors, factor > 1.0 creates wasted GPU work.",
        "d3d11.metalSpatialUpscaleFactor = 1.0",
        "",
        "# Handle Cmd+Tab gracefully in fullscreen exclusive mode.",
        "dxgi.handleAltTab = True",
      ].join("\n")
    );
  }
}

const CURRENT_RESHADE_VERSION = "5.8.0";

export async function* checkAndDownloadReshade(
  aria2: Aria2,
  wine: Wine,
  gameDir: string
): CommonUpdateProgram {
  const reshaderDir = resolve("./reshade");

  if (
    eq(
      CURRENT_RESHADE_VERSION,
      await getKeyOrDefault("installed_reshade", "0.0.0")
    )
  ) {
    return;
  }

  await mkdirp(reshaderDir);
  await mkdirp(join(reshaderDir, "Shaders"));
  await mkdirp(join(reshaderDir, "Textures"));
  yield ["setStateText", "DOWNLOADING_ENVIRONMENT"];
  for await (const progress of aria2.doStreamingDownload({
    uri: `https://reshade.me/downloads/ReShade_Setup_${CURRENT_RESHADE_VERSION}_Addon.exe`,
    absDst: join(reshaderDir, "install.exe"),
  })) {
    yield [
      "setProgress",
      Number((progress.completedLength * BigInt(100)) / progress.totalLength),
    ];
    yield [
      "setStateText",
      "DOWNLOADING_ENVIRONMENT_SPEED",
      `${humanFileSize(Number(progress.downloadSpeed))}`,
    ];
  }
  for await (const progress of aria2.doStreamingDownload({
    uri: `https://lutris.net/files/tools/dll/d3dcompiler_47.dll`,
    absDst: join(reshaderDir, "d3dcompiler_47.dll"),
  })) {
    yield [
      "setProgress",
      Number((progress.completedLength * BigInt(100)) / progress.totalLength),
    ];
    yield [
      "setStateText",
      "DOWNLOADING_ENVIRONMENT_SPEED",
      `${humanFileSize(Number(progress.downloadSpeed))}`,
    ];
  }
  yield ["setStateText", "EXTRACT_ENVIRONMENT"];
  yield ["setUndeterminedProgress"];
  const b = await readBinary(join(reshaderDir, "install.exe"));
  const s = new Uint8Array(b);
  const offset = s.findIndex((v, idx, arr) => {
    return (
      v == 0x50 &&
      arr[idx + 1] == 0x4b &&
      arr[idx + 2] == 0x03 &&
      arr[idx + 3] == 0x04
    );
  });
  await writeBinary(join(reshaderDir, "install.zip"), b.slice(offset));

  for await (const [dec, total] of doStreamUnzip(
    join(reshaderDir, "install.zip"),
    reshaderDir
  )) {
    yield ["setProgress", (dec / total) * 100];
  }

  await forceMove(
    join(reshaderDir, "ReShade64.dll"),
    join(reshaderDir, "dxgi.dll")
  );

  await writeFile(
    join(gameDir, "ReShade.ini"),
    `[GENERAL]
EffectSearchPaths=${wine.toWinePath(resolve("./reshade/Shaders"))}
TextureSearchPaths=${wine.toWinePath(resolve("./reshade/Textures"))}`
  );

  await setKey("installed_reshade", CURRENT_RESHADE_VERSION);
}
