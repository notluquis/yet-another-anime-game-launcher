import { FormControl, FormLabel, Box, Checkbox, Text } from "../components/ui";
import { createEffect, createSignal } from "solid-js";
import { Locale } from "@locale";
import { assertValueDefined, getKey, setKey } from "@utils";
import { Config, NOOP } from "./config-def";

declare module "./config-def" {
  interface Config {
    captureDisplaysForFullscreen: boolean;
  }
}

export async function createCaptureDisplaysFullscreenConfig({
  locale,
  config,
}: {
  config: Partial<Config>;
  locale: Locale;
}) {
  try {
    config.captureDisplaysForFullscreen =
      (await getKey("config_captureDisplaysForFullscreen")) == "true";
  } catch {
    config.captureDisplaysForFullscreen = false;
  }

  const [value, setValue] = createSignal(config.captureDisplaysForFullscreen);

  async function onSave(apply: boolean) {
    assertValueDefined(config.captureDisplaysForFullscreen);
    if (!apply) {
      setValue(config.captureDisplaysForFullscreen);
      return NOOP;
    }
    if (config.captureDisplaysForFullscreen == value()) return NOOP;
    config.captureDisplaysForFullscreen = value();
    await setKey(
      "config_captureDisplaysForFullscreen",
      config.captureDisplaysForFullscreen ? "true" : "false"
    );
    return NOOP;
  }

  createEffect(() => {
    value();
    onSave(true);
  });

  return [
    function UI() {
      return (
        <FormControl id="captureDisplaysForFullscreen">
          <FormLabel>
            {locale.get("SETTING_CAPTURE_DISPLAYS_FULLSCREEN")}
          </FormLabel>
          <Box>
            <Checkbox
              checked={value()}
              size="md"
              onChange={() => setValue(x => !x)}
            >
              {locale.get("SETTING_ENABLED")}
            </Checkbox>
          </Box>
          <Text class="text-xs text-gray-400 mt-1">
            {locale.get("SETTING_CAPTURE_DISPLAYS_FULLSCREEN_DESC")}
          </Text>
        </FormControl>
      );
    },
  ] as const;
}
