import { Aria2 } from "./aria2";
import { Github, GithubReleaseInfo } from "./github";
import { gt } from "semver";
import { CURRENT_YAAGL_VERSION } from "./constants";
import {
  forceMove,
  log,
  resolve,
  rmrf_dangerously,
  mkdirp,
  tar_extract_directory,
  removeFile,
} from "./utils";
import { CommonUpdateProgram } from "./common-update-ui";

const owner = "3shain";
const repo = "yet-another-anime-game-launcher";

export async function createUpdater(deps: { github: Github; aria2: Aria2 }) {
  if (CURRENT_YAAGL_VERSION === "development") {
    return {
      latest: true,
    } as const;
  }
  try {
    const latest: GithubReleaseInfo = (await deps.github.api(
      `/repos/${owner}/${repo}/releases/latest`
    )) as GithubReleaseInfo;
    const neu = latest.assets.find(x => x.name == "resources_hk4eos.neu");
    const sidecar = latest.assets.find(x => x.name == "Yaagl.OS.app.tar.gz");

    if (gt(latest.tag_name, CURRENT_YAAGL_VERSION) && neu !== undefined) {
      return {
        latest: false,
        downloadUrl: neu.browser_download_url,
        sidecarDownloadUrl: sidecar?.browser_download_url,
        version: latest.tag_name,
        description: latest.body,
      } as const;
    }
    return {
      latest: true,
    } as const;
  } catch {
    return {
      latest: undefined,
    };
  }
}

export type Updater = ReturnType<typeof createUpdater> extends Promise<infer T>
  ? T
  : never;

export async function* downloadProgram(
  aria2: Aria2,
  url: string,
  sidecarUrl?: string
): CommonUpdateProgram {
  yield ["setStateText", "DOWNLOADING_UPDATE_FILE"];
  if (sidecarUrl) {
    for await (const progress of aria2.doStreamingDownload({
      uri: sidecarUrl,
      absDst: resolve("./sidecar.tar.gz"),
    })) {
      yield [
        "setProgress",
        Number((progress.completedLength * BigInt(50)) / progress.totalLength),
      ];
    }
    await rmrf_dangerously("./sidecar");
    await mkdirp("./sidecar");
    const topLevelDir = "Yaagl OS.app";

    await tar_extract_directory(
      "./sidecar.tar.gz",
      "./sidecar",
      `${topLevelDir}/Contents/Resources/sidecar`,
      false
    );

    await removeFile("./sidecar.tar.gz");
  }

  for await (const progress of aria2.doStreamingDownload({
    uri: url,
    absDst: resolve("./resources.neu.update"),
  })) {
    yield [
      "setProgress",
      sidecarUrl
        ? 50 +
          Number((progress.completedLength * BigInt(50)) / progress.totalLength)
        : Number(
            (progress.completedLength * BigInt(100)) / progress.totalLength
          ),
    ];
  }

  yield ["setUndeterminedProgress"];
  await forceMove("./resources.neu.update", "./resources.neu");
}
