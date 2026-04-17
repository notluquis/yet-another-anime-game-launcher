import { FormControl, FormLabel, Select, Text } from "../components/ui";
import { createEffect, createSignal } from "solid-js";
import { Locale } from "@locale";
import {
  assertValueDefined,
  fileOrDirExists,
  getKey,
  readAllLinesIfExists,
  resolve,
  setKey,
  writeFile,
} from "@utils";
import { Config, NOOP } from "./config-def";

declare module "./config-def" {
  interface Config {
    metalfxSpatialFactor: string;
  }
}

const OPTIONS = [
  { value: "1.0", labelKey: "SETTING_METALFX_SPATIAL_OFF" },
  { value: "1.3", labelKey: "SETTING_METALFX_SPATIAL_QUALITY" },
  { value: "1.5", labelKey: "SETTING_METALFX_SPATIAL_BALANCE" },
  { value: "2.0", labelKey: "SETTING_METALFX_SPATIAL_PERFORMANCE" },
] as const;

const CONFIG_KEY = "d3d11.metalSpatialUpscaleFactor";

async function updateDxmtConfFactor(factor: string) {
  const path = resolve("./dxmt.conf");
  const exists = await fileOrDirExists(path);
  const lines = exists ? await readAllLinesIfExists(path) : [];

  const keyRegex = new RegExp(
    `^\\s*${CONFIG_KEY.replace(/\./g, "\\.")}\\s*=`
  );
  const newLine = `${CONFIG_KEY} = ${factor}`;
  let replaced = false;
  const next = lines.map(line => {
    if (keyRegex.test(line)) {
      replaced = true;
      return newLine;
    }
    return line;
  });
  if (!replaced) {
    if (next.length > 0 && next[next.length - 1] !== "") next.push("");
    next.push(newLine);
  }
  await writeFile(path, next.join("\n"));
}

export async function createMetalFXSpatialConfig({
  locale,
  config,
}: {
  config: Partial<Config>;
  locale: Locale;
}) {
  try {
    const stored = await getKey("config_metalfxSpatialFactor");
    config.metalfxSpatialFactor = stored || "1.0";
  } catch {
    config.metalfxSpatialFactor = "1.0";
  }

  const [value, setValue] = createSignal(config.metalfxSpatialFactor);

  async function onSave(apply: boolean) {
    assertValueDefined(config.metalfxSpatialFactor);
    if (!apply) {
      setValue(config.metalfxSpatialFactor);
      return NOOP;
    }
    if (config.metalfxSpatialFactor == value()) return NOOP;
    config.metalfxSpatialFactor = value();
    await setKey("config_metalfxSpatialFactor", config.metalfxSpatialFactor);
    await updateDxmtConfFactor(config.metalfxSpatialFactor);
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
        <FormControl id="metalfxSpatial">
          <FormLabel>{locale.get("SETTING_METALFX_SPATIAL")}</FormLabel>
          <Select
            value={options.find(opt => opt.value === value()) ?? null}
            onChange={(opt: { value: string; label: string } | null) =>
              opt && setValue(opt.value)
            }
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
              <Select.Value<{ value: string; label: string }>>
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
            {locale.get("SETTING_METALFX_SPATIAL_DESC")}
          </Text>
        </FormControl>
      );
    },
  ] as const;
}
