import { CommonUpdateProgram } from "@common-update-ui";
import { Locale } from "@locale";
import { fatal, alert, prompt } from "@utils";
import { logError } from "../utils/structured-logging";
import {
  isFileNotFoundError,
  isPermissionError,
} from "../utils/external-drive";
import { createSignal } from "solid-js";

function formatEta(secs: number): string {
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s`;
  return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
}

export function createTaskQueueState({ locale }: { locale: Locale }) {
  const [statusText, setStatusText] = createSignal("");
  const [progress, setProgress] = createSignal(0);
  const [programBusy, setBusy] = createSignal(false);

  const taskQueue: AsyncGenerator<unknown, void, () => CommonUpdateProgram> =
    (async function* () {
      while (true) {
        const task = yield 0;
        setBusy(true);

        // Inner retry loop: task is a factory (() => CommonUpdateProgram),
        // so we can call task() again to restart the same operation.
        let retry = true;
        while (retry) {
          retry = false;
          try {
            for await (const text of task()) {
              switch (text[0]) {
                case "setProgress":
                  setProgress(text[1]);
                  break;
                case "setUndeterminedProgress":
                  setProgress(0);
                  break;
                case "setStateText": {
                  let formatted = locale.format(text[1], text.slice(2));
                  if (
                    text[1] === "DOWNLOADING_FILE_PROGRESS" &&
                    text.length > 10
                  ) {
                    // text[2..] = [filename, netSpeed, downloaded, total, diskSpeed, isDiskBottleneck, speedRaw, downloadedRaw, totalRaw]
                    const speedRaw = Number(text[8]);
                    const downloadedRaw = Number(text[9]);
                    const totalRaw = Number(text[10]);
                    if (speedRaw > 0 && totalRaw > downloadedRaw) {
                      const etaSecs = Math.floor(
                        (totalRaw - downloadedRaw) / speedRaw
                      );
                      formatted += ` — ~${formatEta(etaSecs)}`;
                    }
                  }
                  setStatusText(formatted);
                  break;
                }
              }
            }
          } catch (e) {
            const errorMessage = e instanceof Error ? e.message : String(e);
            const isFileNotFound = isFileNotFoundError(e);
            const isPermissionDenied = isPermissionError(e);
            const isEmptyDirError =
              errorMessage.includes("not empty") ||
              errorMessage.includes("ENOTEMPTY") ||
              errorMessage.includes("directory is not empty");

            setStatusText("");
            setProgress(0);

            // External drive disconnection — offer real retry
            if (isFileNotFound) {
              await logError("External drive disconnected", e, {
                operation: "game install/launch/repair",
                errorType: "file_not_found",
                recoverable: true,
              });

              const shouldRetry = await prompt(
                "External Drive Disconnected",
                `The game installation directory is no longer accessible.\n\n` +
                  `This typically means your external hard drive was disconnected.\n\n` +
                  `Reconnect the drive, then click Yes to retry.\n` +
                  `Click No to cancel the operation.\n\n` +
                  `Error: ${errorMessage}`
              );

              if (shouldRetry) {
                retry = true; // restart task() from the top
              }
              continue;
            }

            // Permission denied
            if (isPermissionDenied) {
              await logError("Permission denied during operation", e, {
                operation: "game install/launch/repair",
                errorType: "permission_denied",
                recoverable: true,
              });

              const shouldRetry = await prompt(
                "Permission Denied",
                `Cannot access the game installation directory.\n\n` +
                  `Please verify you have read/write access, then click Yes to retry.\n` +
                  `Click No to cancel.\n\n` +
                  `Error: ${errorMessage}`
              );

              if (shouldRetry) {
                retry = true;
              }
              continue;
            }

            // Directory not empty
            if (isEmptyDirError) {
              await logError("Directory not empty error", e, {
                operation: "game installation",
                errorType: "dir_not_empty",
                recoverable: true,
              });

              await alert(
                "Directory Not Empty",
                `Cannot proceed: The installation directory is not empty.\n\n` +
                  `Please choose an empty directory and try again.`
              );
              continue;
            }

            // Unknown / fatal
            await logError("Fatal operation error", e, {
              operation: "game install/launch/repair",
              errorType: "unknown",
              recoverable: false,
            });
            await fatal(e);
            return;
          }
        }

        setBusy(false);
      }
    })();
  taskQueue.next(); // ignored anyway

  return [statusText, progress, programBusy, taskQueue] as const;
}
