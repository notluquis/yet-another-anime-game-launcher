import { Locale } from "@locale";
import { Server } from "@constants";
import {
  HoyoConnectGameBackground,
  HoyoConnectGameBackgroundType,
  HoyoConnectGetAllGameBasicInfoResponse,
} from "./launcher-info";
import { exec } from "@utils";

async function fetch(url: string) {
  const { stdOut } = await exec(["curl", url]);
  return {
    async json() {
      return JSON.parse(stdOut);
    },
  };
}

export async function getLatestAdvInfo(
  locale: Locale,
  server: Server
): Promise<HoyoConnectGameBackground> {
  const ret: HoyoConnectGetAllGameBasicInfoResponse = await (
    await fetch(
      server.adv_url + `&language=${locale.get("CONTENT_LANG_ID")}`
    )
  ).json();
  const game = ret.data.game_info_list.find(x => x.game.biz === server.id);
  if (!game || game.backgrounds.length < 1)
    throw new Error(`failed to fetch game information: ${server.id}`);

  const sortedBackgrounds = game.backgrounds.sort((a, b) => {
    const isAVideo =
      a.type === HoyoConnectGameBackgroundType.BACKGROUND_TYPE_VIDEO;
    const isBVideo =
      b.type === HoyoConnectGameBackgroundType.BACKGROUND_TYPE_VIDEO;

    if (isAVideo && !isBVideo) return -1;
    if (!isAVideo && isBVideo) return 1;
    return 0;
  });
  return sortedBackgrounds[0];
}
