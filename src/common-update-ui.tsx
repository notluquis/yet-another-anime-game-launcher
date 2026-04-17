import {
  Button,
  Progress,
  ProgressIndicator,
  Center,
  Box,
  VStack,
  Image,
} from "./components/ui";
import { SpeedIndicator } from "./components/speed-indicators";
import { createSignal, onMount, Show } from "solid-js";
import { fatal, _safeRelaunch } from "./utils";
import { Locale, LocaleTextKey } from "./locale";
import { UPDATE_UI_IMAGE } from "./clients";

function formatEta(secs: number): string {
  if (secs < 60) return `${secs}s`;
  if (secs < 3600) return `${Math.floor(secs / 60)}m ${secs % 60}s`;
  return `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
}

export function createCommonUpdateUI(
  locale: Locale,
  program: () => CommonUpdateProgram
) {
  let confirmRestart: (v: unknown) => void;
  const confirmRestartPromise = new Promise(res => {
    confirmRestart = res;
  });
  return function CommonUpdateUI() {
    const [progress, setProgress] = createSignal(0);
    const [statusText, setStatusText] = createSignal("");
    const [downloadInfo, setDownloadInfo] = createSignal<{
      filename: string;
      downloaded: string;
      total: string;
      networkSpeed: string;
      diskSpeed: string;
      isDiskBottleneck: boolean;
      speedRaw: number;
      downloadedRaw: number;
      totalRaw: number;
    } | null>(null);
    const [done, setDone] = createSignal(false);

    const eta = () => {
      const info = downloadInfo();
      if (!info || info.speedRaw <= 0 || info.totalRaw <= 0) return null;
      const remaining = info.totalRaw - info.downloadedRaw;
      if (remaining <= 0) return null;
      return formatEta(Math.floor(remaining / info.speedRaw));
    };

    onMount(() => {
      (async () => {
        for await (const text of program()) {
          switch (text[0]) {
            case "setProgress":
              setProgress(text[1]);
              break;
            case "setUndeterminedProgress":
              setProgress(0);
              setDownloadInfo(null);
              break;
            case "setStateText":
              if (text[1] === "DOWNLOADING_FILE_PROGRESS" && text.length >= 8) {
                setDownloadInfo({
                  filename: text[2] as string,
                  downloaded: text[4] as string,
                  total: text[5] as string,
                  networkSpeed: text[3] as string,
                  diskSpeed: text[6] as string,
                  isDiskBottleneck: text[7] === "true",
                  speedRaw: text.length > 8 ? Number(text[8]) : 0,
                  downloadedRaw: text.length > 9 ? Number(text[9]) : 0,
                  totalRaw: text.length > 10 ? Number(text[10]) : 0,
                });
                setStatusText(locale.get("DOWNLOADING"));
              } else {
                setStatusText(locale.format(text[1], text.slice(2)));
                setDownloadInfo(null);
              }
              break;
          }
        }
        setDone(true);
        await confirmRestartPromise;
        await _safeRelaunch();
      })()
        .then()
        .catch(fatal);
    });

    return (
      <div
        class="w-screen h-screen flex items-center justify-center"
        style={{
          background: `
            linear-gradient(135deg, rgba(30, 58, 138, 0.8) 0%, rgba(79, 70, 229, 0.7) 25%, rgba(139, 92, 246, 0.6) 50%, rgba(88, 86, 214, 0.7) 75%, rgba(30, 58, 138, 0.8) 100%)
          `,
          "background-size": "400% 400%",
          animation: "gradientShift 15s ease infinite",
        }}
      >
        <style>
          {`
            @keyframes gradientShift {
              0% { background-position: 0% 50%; }
              50% { background-position: 100% 50%; }
              100% { background-position: 0% 50%; }
            }
            @keyframes pulse-glow {
              0%, 100% {
                opacity: 0.8;
                filter: drop-shadow(0 0 20px rgba(59, 130, 246, 0.4));
              }
              50% {
                opacity: 1;
                filter: drop-shadow(0 0 30px rgba(59, 130, 246, 0.6));
              }
            }
          `}
        </style>

        <VStack alignItems="stretch" spacing="$8" w="min(90vw, 600px)" class="relative z-10">
          {/* Logo/Character Image */}
          <Center>
            <div
              class="relative"
              style={{
                animation: "pulse-glow 3s ease-in-out infinite",
              }}
            >
              <div class="absolute inset-0 bg-linear-to-r from-blue-500 via-purple-500 to-pink-500 rounded-2xl blur-2xl opacity-30"></div>
              <div class="relative bg-linear-to-b from-white/10 to-white/5 backdrop-blur-lg rounded-2xl p-4 border border-white/20">
                <Image boxSize={280} src={UPDATE_UI_IMAGE} class="drop-shadow-lg" />
              </div>
            </div>
          </Center>

          {/* Status Card */}
          <div class="bg-linear-to-r from-black/40 to-black/20 backdrop-blur-md rounded-2xl p-6 border border-white/10 shadow-2xl">
            <Show
              when={downloadInfo()}
              fallback={
                <div style="text-align: center">
                  <h1 class="text-2xl font-bold bg-linear-to-r from-blue-200 via-purple-200 to-pink-200 bg-clip-text text-transparent drop-shadow-lg">
                    {statusText()}
                  </h1>
                </div>
              }
            >
              {(info) => (
                <div>
                  {/* Header row: status + ETA */}
                  <div class="flex items-baseline justify-between mb-3">
                    <h1 class="text-xl font-bold bg-linear-to-r from-blue-200 via-purple-200 to-pink-200 bg-clip-text text-transparent drop-shadow-lg">
                      {statusText()}
                    </h1>
                    <Show when={eta()}>
                      {(t) => (
                        <span class="text-sm text-white/50 font-mono">
                          ~{t()} remaining
                        </span>
                      )}
                    </Show>
                  </div>

                  {/* Filename */}
                  <div
                    class="text-xs text-white/40 truncate mb-3"
                    title={info().filename}
                  >
                    {info().filename}
                  </div>

                  {/* Downloaded / Total */}
                  <div class="flex items-center justify-between mb-4">
                    <span class="text-sm text-white/70 font-mono">
                      {info().downloaded}
                      <span class="text-white/40"> / {info().total}</span>
                    </span>
                  </div>

                  {/* Speed indicators */}
                  <SpeedIndicator
                    networkSpeed={info().networkSpeed}
                    diskSpeed={info().diskSpeed}
                    isDiskBottleneck={info().isDiskBottleneck}
                    iconSize="md"
                    class="justify-center"
                  />
                </div>
              )}
            </Show>
          </div>

          {/* Progress Bar */}
          <Box class="backdrop-blur-sm bg-white/5 rounded-2xl p-6 border border-white/10">
            <Show
              when={!done()}
              fallback={
                <Center>
                  <Button
                    onClick={confirmRestart}
                    class="bg-linear-to-r from-blue-600 to-purple-600 hover:from-blue-700 hover:to-purple-700 text-white font-bold py-3 px-8 rounded-lg transition-all duration-200 shadow-lg hover:shadow-xl"
                  >
                    {locale.get("RESTART_TO_INSTALL")}
                  </Button>
                </Center>
              }
            >
              <div>
                <div class="text-sm text-white/60 mb-3 font-semibold">
                  {progress() > 0 ? `${Math.round(progress())}%` : ""}
                </div>
                <Progress
                  value={progress()}
                  indeterminate={progress() == 0}
                  class="bg-linear-to-r from-blue-900/30 to-purple-900/30 rounded-full overflow-hidden"
                >
                  <ProgressIndicator
                    animated
                    striped
                    class="bg-linear-to-r from-blue-500 to-purple-500 shadow-lg shadow-blue-500/30"
                  />
                </Progress>
              </div>
            </Show>
          </Box>
        </VStack>
      </div>
    );
  };
}

export type CommonUpdateProgram<Ret = void> = AsyncGenerator<
  CommonProgressUICommand,
  Ret
>;

export type CommonProgressUICommand =
  | ["setProgress", number]
  | ["setStateText", LocaleTextKey, ...string[]]
  | ["setUndeterminedProgress"];
