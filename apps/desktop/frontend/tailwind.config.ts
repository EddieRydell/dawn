import type { Config } from "tailwindcss";

export default {
  content: ["./index.html", "./src/**/*.{ts,tsx}"],
  theme: {
    extend: {
      colors: {
        dawn: {
          bg: "#16171a",
          panel: "#202226",
          rail: "#272a2f",
          line: "#373b42",
          text: "#ebe7df",
          muted: "#a8a29a",
          accent: "#6abf8a",
          warn: "#e3a84f",
          error: "#df6b6b"
        }
      }
    }
  },
  plugins: []
} satisfies Config;
