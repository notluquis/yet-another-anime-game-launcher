import { log, timeout } from "./utils";

// GitHub proxy mirrors - direct first, then proxies as fallback
const END_POINTS = [
  "", // Direct connection (preferred)
  "https://ghp.3shain.uk/", // Original proxy
  "https://gh-proxy.com/", // Alternative proxies
  "https://ghfast.top/",
  "https://gh.llkk.cc/",
];

// Cached result to avoid re-checking on subsequent calls
let cachedEndpoint: string | null = null;

export async function createGithubEndpoint() {
  // Return cached result if available
  if (cachedEndpoint !== null) {
    await log(`Using cached github endpoint: ${cachedEndpoint || "direct"}`);
    return createEndpointResult(cachedEndpoint);
  }

  await log(`Checking github endpoints`);

  for (let attempt = 0; attempt < 3; attempt++) {
    try {
      // Race all endpoints - first successful response wins
      // Failed fetches return never-resolving promise to not block the race
      cachedEndpoint = (await Promise.race([
        ...END_POINTS.map(
          prefix =>
            fetch(`${prefix}https://api.github.com/octocat`)
              .then(x => {
                if (!x.ok) throw new Error(`HTTP ${x.status}`);
                return x.text();
              })
              .then(() => prefix)
              .catch(() => new Promise(() => {})) // Never resolve on error
        ),
        timeout(10000).then(() => {
          throw new Error("Timeout");
        }),
      ])) as string;
      break;
    } catch (e) {
      await log(
        `GitHub endpoint check failed (attempt ${attempt + 1}/3): ${e}`
      );
      if (attempt < 2) {
        await new Promise(r => setTimeout(r, 1000));
      }
    }
  }

  if (cachedEndpoint === null) {
    throw new Error("Failed to connect to GitHub after 3 attempts");
  }

  await log(`Using github endpoint: ${cachedEndpoint || "direct"}`);
  return createEndpointResult(cachedEndpoint);
}

function createEndpointResult(prefix: string) {
  function api(path: `/${string}`): Promise<unknown> {
    return fetch(`${prefix}https://api.github.com${path}`).then(x => {
      if (x.status == 200 || x.status == 301 || x.status == 302) {
        return x.json();
      }
      return Promise.reject(
        new Error(`Request failed: ${x.status} ${x.statusText} (${x.url})`)
      );
    });
  }

  function acceleratedPath(path: string) {
    return `${prefix}${path}`;
  }

  return {
    api,
    acceleratedPath,
    mirrorURL: prefix, // Expose for aria2 downloads
  };
}

export type Github = ReturnType<typeof createGithubEndpoint> extends Promise<
  infer T
>
  ? T
  : never;

export interface GithubReleaseInfo {
  url: string;
  html_url: string;
  assets_url: string;
  id: number;
  tag_name: string;
  name: string;
  body: string;
  draft: boolean;
  prerelease: boolean;
  created_at: string;
  published_at: string;
  author: unknown;
  assets: GithubReleaseAssetsInfo[];
}

export interface GithubReleaseAssetsInfo {
  url: string;
  browser_download_url: string;
  id: number;
  name: string;
  content_type: string;
}

export type GithubReleases = GithubReleaseInfo[];
