import { basename } from "path-browserify";
import { Sophon, resolveSophonTempdir } from "@sophon";
import { CommonUpdateProgram } from "@common-update-ui";
import { log, humanFileSize } from "@utils";

function formatEta(secs: number): string {
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s`;
  return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
}

export async function* checkIntegrityProgram({
  sophon,
  gameDir,
}: {
  gameDir: string;
  sophon: Sophon;
}): CommonUpdateProgram {
  const taskId = await sophon.startRepair({
    gamedir: gameDir,
    game_type: "hk4e",
    tempdir: await resolveSophonTempdir(gameDir),
    repair_mode: "smart",
  });

  // Indeterminate bar while the server downloads the manifest
  yield ["setUndeterminedProgress"];
  yield ["setStateText", "SCANNING_FILES", "0", "...", ""];

  for await (const progress of sophon.streamOperationProgress(taskId)) {
    switch (progress.type) {
      // Manifest ready — update total so "0/..." becomes "0/2566"
      case "repair_summary":
        yield ["setStateText", "SCANNING_FILES", "0", String(progress.total_files), ""];
        break;

      case "check_file": {
        const op = progress.overall_progress;
        const etaStr =
          op.eta_secs != null ? ` — ~${formatEta(Number(op.eta_secs))}` : "";
        yield [
          "setStateText",
          "SCANNING_FILES",
          String(op.checked_files),
          String(op.total_files),
          etaStr,
        ];
        yield ["setProgress", Number(op.overall_percent)];
        break;
      }

      case "chunk_progress":
        log(`Chunk progress: ${progress.chunk_size} bytes downloaded`);
        yield [
          "setStateText",
          "DOWNLOADING_FILE_PROGRESS",
          basename(progress.filename),
          humanFileSize(progress.overall_progress.download_speed),
          humanFileSize(progress.overall_progress.downloaded_size),
          humanFileSize(progress.overall_progress.total_size),
          humanFileSize(progress.overall_progress.write_speed ?? 0),
          String(
            (progress.overall_progress.write_speed ?? 0) > 0 &&
            (progress.overall_progress.write_speed ?? 0) <
              progress.overall_progress.download_speed
          ),
          String(progress.overall_progress.download_speed),
          String(progress.overall_progress.downloaded_size),
          String(progress.overall_progress.total_size),
        ];

        yield [
          "setProgress",
          Number(progress.overall_progress.overall_percent),
        ];
        break;

      default:
        break;
    }
  }
}
