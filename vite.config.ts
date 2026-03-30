/// <reference types="vitest" />
import { defineConfig } from "vite";
import solidPlugin from "vite-plugin-solid";
import tailwindcss from "@tailwindcss/vite";

// https://vitejs.dev/config/
export default defineConfig({
  resolve: {
    tsconfigPaths: true,
  },
  plugins: [
    tailwindcss(),
    {
      name: "channel-client switcher",
      load: (id) => {
        if (id.endsWith("/src/clients/index.ts")) {
          const cc = process.env["YAAGL_CHANNEL_CLIENT"] ?? "hk4eos";
          console.info(`Building channel client ${cc}`);
          return { code: `export * from './${cc}'`, moduleType: "js" };
        }
        return null;
      },
    },
    solidPlugin(),
  ],

  envPrefix: ["VITE_", "YAAGL_"],
  build: {
    target: "esnext",
    minify: true,
    sourcemap: false,
    outDir: "dist",
    rolldownOptions: {},
  },
  test: {
    include: ["src/**/*.spec.ts"],
    environment: "node",
  },
});
