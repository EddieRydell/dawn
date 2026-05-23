export type Diagnostic = {
  severity: "Error" | "Warning";
  path: string;
  message: string;
};

export type LanguageProblem = {
  path: string;
  message: string;
  severity: "Error" | "Warning" | "Info" | "Hint";
  source?: string;
  code?: string;
  line: number;
  column: number;
  endLine: number;
  endColumn: number;
};

export type ProjectEntry = {
  path: string;
  kind: "directory" | "file";
};

export type ProjectState = {
  root: string;
  files: string[];
  entries: ProjectEntry[];
  diagnostics: Diagnostic[];
};

export type FrameSummary = {
  pixels: number;
  fixtureSpans: number;
  warnings?: string[];
};

export type FileMove = {
  oldPath: string;
  newPath: string;
};

export type FileOperationState = {
  project: ProjectState;
  moved: FileMove[];
};
