import { defineConfig } from "@natsuneko-laboratory/makit";
import cloudflare from "@natsuneko-laboratory/makit-adapter-cloudflare-pages";

export default defineConfig({
  title: "Symphra",
  description: "A programming language for composing complete musical works as text and rendering them offline to WAV.",
  lang: "en-US",
  siteUrl: "https://symphra.natsuneko.com",
  sourceDir: "docs",
  publicDir: "public",
  outDir: "dist",
  header: {
    title: "Symphra",
    links: [
      { label: "Introduction", href: "/introduction/what-is-symphra/" },
      { label: "Language", href: "/language/overview/" },
      { label: "Reference", href: "/reference/grammar/" },
      {
        label: "GitHub",
        href: "https://github.com/mika-f/Symphra",
        external: true,
      },
    ],
  },
  footer: {
    copyright: "© 2026 Kanon Mochizuki · MIT License",
    links: [
      {
        label: "GitHub",
        href: "https://github.com/mika-f/Symphra",
        external: true,
      },
    ],
  },
  markdown: {
    shiki: {
      themes: {
        light: "github-light",
        dark: "github-dark",
      },
      languages: [
        async () => (await import("./editors/vscode/syntaxes/symphra.tmLanguage.json", { with: { type: "json" } })).default
      ],
      unknownLanguage: "plain-text",
    },
  },
  navigation: {
    pagination: {
      enabled: true,
      crossSection: true,
    },
  },
  dev: {
    silentNext: true,
  },
  deployment: {
    adapter: cloudflare({
      projectName: "symphra-natsuneko-com",
      generateWranglerConfig: true,
    }),
  }
});
