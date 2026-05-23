import * as monaco from "monaco-editor";

const DAWN_YAML_LANGUAGE_ID = "dawn-yaml";

let dawnLanguageRegistered = false;

export function ensureDawnLanguageRegistered() {
  if (!monaco.languages.getLanguages().some((language) => language.id === DAWN_YAML_LANGUAGE_ID)) {
    monaco.languages.register({
      id: DAWN_YAML_LANGUAGE_ID,
      extensions: [".dawn"],
      aliases: ["Dawn", "dawn"]
    });
  }

  if (!dawnLanguageRegistered) {
    monaco.languages.setMonarchTokensProvider(DAWN_YAML_LANGUAGE_ID, dawnYamlMonarch);
    monaco.editor.defineTheme("dawn-dark", {
      base: "vs-dark",
      inherit: true,
      rules: [
        { token: "key", foreground: "7dd3fc" },
        { token: "string", foreground: "fde68a" },
        { token: "number", foreground: "f0abfc" }
      ],
      colors: {
        "editor.background": "#101317"
      }
    });
    dawnLanguageRegistered = true;
  }
}

export function dawnLanguageIdForPath(path: string) {
  return path.endsWith(".dawn") ? DAWN_YAML_LANGUAGE_ID : "plaintext";
}

const dawnYamlMonarch: monaco.languages.IMonarchLanguage = {
  tokenizer: {
    root: [
      [/#.*$/, "comment"],
      [/^[ \t-]*([A-Za-z_][A-Za-z0-9_]*)(?=\s*:)/, "key"],
      [/"([^"\\]|\\.)*"/, "string"],
      [/'[^']*'/, "string"],
      [/#[0-9A-Fa-f]{6}\b/, "number.hex"],
      [/\b\d+(\.\d+)?(ms|s|m)?\b/, "number"],
      [/\b(true|false|null)\b/, "keyword"],
      [/[{}\[\],:]/, "delimiter"]
    ]
  }
};
