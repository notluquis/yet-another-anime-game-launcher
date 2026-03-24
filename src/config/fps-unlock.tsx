import {
  FormControl,
  FormLabel,
  Select,
} from "../components/ui";
import { createEffect, createSignal } from "solid-js";
import { Locale } from "../locale";
import { getKey, setKey, _safeRelaunch, assertValueDefined } from "../utils";
import { Config } from "./config-def";

declare module "./config-def" {
  interface Config {
    fpsUnlock: "default" | "120" | "144";
  }
}

const CONFIG_KEY = "config_fps_unlock";

export default async function ({
  locale,
  config,
}: {
  locale: Locale;
  config: Partial<Config>;
}) {
  try {
    config.fpsUnlock = (await getKey(CONFIG_KEY)) as "default" | "120" | "144";
  } catch {
    config.fpsUnlock = "default"; // default value
  }

  const [value, setValue] = createSignal(config.fpsUnlock);

  async function onSave(apply: boolean) {
    assertValueDefined(config.fpsUnlock);
    if (!apply) {
      setValue(config.fpsUnlock);
      return;
    }
    if (config.fpsUnlock == value()) return;
    config.fpsUnlock = value();
    await setKey(CONFIG_KEY, config.fpsUnlock);
    return;
  }

  createEffect(() => {
    value();
    onSave(true);
  });

  const options = [
    { value: "default", label: locale.get("SETTING_FPS_UNLOCK_DEFAULT") },
    { value: "120", label: "120Hz" },
    { value: "144", label: "144Hz" },
  ];

  return [
    function UI() {
      return [
        <FormControl>
          <FormLabel>{locale.get("SETTING_FPS_UNLOCK")}</FormLabel>
          <Select
            value={options.find(opt => opt.value === value()) ?? null}
            onChange={(opt: { value: string; label: string } | null) => opt && setValue(opt.value as any)}
            options={options}
            optionValue="value"
            optionTextValue="label"
            itemComponent={props => (
              <Select.Item item={props.item} class="flex items-center justify-between px-3 py-2 cursor-pointer hover:bg-primary-50 data-highlighted:bg-primary-100 rounded">
                <Select.ItemLabel>{props.item.rawValue.label}</Select.ItemLabel>
                <Select.ItemIndicator class="inline-flex items-center">
                  <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M5 13l4 4L19 7" />
                  </svg>
                </Select.ItemIndicator>
              </Select.Item>
            )}
          >
            <Select.Trigger class="flex items-center justify-between px-3 py-2 bg-gray-800 border border-gray-700 rounded text-white hover:border-primary-500 focus:outline-none focus:border-primary-500 w-full">
              <Select.Value<{ value: string; label: string }>>
                {state => state.selectedOption()?.label ?? ""}
              </Select.Value>
              <Select.Icon class="ml-2">
                <svg class="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
                </svg>
              </Select.Icon>
            </Select.Trigger>
            <Select.Content class="bg-white border-2 border-gray-300 rounded shadow-lg mt-1 max-h-60 overflow-auto z-9999">
              <Select.Listbox class="p-1" />
            </Select.Content>
          </Select>
        </FormControl>,
      ];
    },
  ] as const;
}
