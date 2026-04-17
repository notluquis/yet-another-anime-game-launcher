import { FormControl, FormLabel, Select, Text } from "../components/ui";
import { createEffect, createSignal } from "solid-js";
import { Locale } from "@locale";
import { assertValueDefined, getKey, setKey, LoggingPreset } from "@utils";
import { Config, NOOP } from "./config-def";

declare module "./config-def" {
  interface Config {
    loggingSession: LoggingPreset;
  }
}

const OPTIONS = [
  { value: "off", labelKey: "SETTING_LOGGING_SESSION_OFF" },
  { value: "basic", labelKey: "SETTING_LOGGING_SESSION_BASIC" },
  { value: "deep", labelKey: "SETTING_LOGGING_SESSION_DEEP" },
] as const;

function normalize(v: string | undefined | null): LoggingPreset {
  return v === "basic" || v === "deep" ? v : "off";
}

export async function createLoggingSessionConfig({
  locale,
  config,
}: {
  config: Partial<Config>;
  locale: Locale;
}) {
  try {
    config.loggingSession = normalize(await getKey("config_loggingSession"));
  } catch {
    config.loggingSession = "off";
  }

  const [value, setValue] = createSignal<LoggingPreset>(config.loggingSession);

  async function onSave(apply: boolean) {
    assertValueDefined(config.loggingSession);
    if (!apply) {
      setValue(config.loggingSession);
      return NOOP;
    }
    if (config.loggingSession == value()) return NOOP;
    config.loggingSession = value();
    await setKey("config_loggingSession", config.loggingSession);
    return NOOP;
  }

  createEffect(() => {
    value();
    onSave(true);
  });

  const options = OPTIONS.map(opt => ({
    value: opt.value,
    label: locale.get(opt.labelKey),
  }));

  return [
    function UI() {
      return (
        <FormControl id="loggingSession">
          <FormLabel>{locale.get("SETTING_LOGGING_SESSION")}</FormLabel>
          <Select
            value={options.find(opt => opt.value === value()) ?? null}
            onChange={(
              opt: { value: LoggingPreset; label: string } | null
            ) => opt && setValue(opt.value)}
            options={options}
            optionValue="value"
            optionTextValue="label"
            itemComponent={props => (
              <Select.Item
                item={props.item}
                class="flex items-center justify-between px-3 py-2 cursor-pointer hover:bg-primary-50 data-highlighted:bg-primary-100 rounded"
              >
                <Select.ItemLabel>{props.item.rawValue.label}</Select.ItemLabel>
                <Select.ItemIndicator class="inline-flex items-center">
                  <svg
                    class="w-4 h-4"
                    fill="none"
                    stroke="currentColor"
                    viewBox="0 0 24 24"
                  >
                    <path
                      stroke-linecap="round"
                      stroke-linejoin="round"
                      stroke-width="2"
                      d="M5 13l4 4L19 7"
                    />
                  </svg>
                </Select.ItemIndicator>
              </Select.Item>
            )}
          >
            <Select.Trigger class="flex items-center justify-between px-3 py-2 bg-gray-800 border border-gray-700 rounded text-white hover:border-primary-500 focus:outline-none focus:border-primary-500 w-full">
              <Select.Value<{ value: LoggingPreset; label: string }>>
                {state => state.selectedOption()?.label ?? ""}
              </Select.Value>
              <Select.Icon class="ml-2">
                <svg
                  class="w-4 h-4"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24"
                >
                  <path
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    stroke-width="2"
                    d="M19 9l-7 7-7-7"
                  />
                </svg>
              </Select.Icon>
            </Select.Trigger>
            <Select.Content class="bg-white border-2 border-gray-300 rounded shadow-lg mt-1 max-h-60 overflow-auto z-9999">
              <Select.Listbox class="p-1" />
            </Select.Content>
          </Select>
          <Text class="text-xs text-gray-400 mt-1">
            {locale.get("SETTING_LOGGING_SESSION_DESC")}
          </Text>
        </FormControl>
      );
    },
  ] as const;
}
