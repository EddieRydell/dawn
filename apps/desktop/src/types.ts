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

export type AnalysisState = {
  diagnostics: LanguageProblem[];
  resolved: boolean;
  reachableFileCount: number;
  objectCount: number;
};

export type ProjectEntry = {
  path: string;
  kind: "directory" | "file";
};

export type ProjectState = {
  root: string;
  files: string[];
  entries: ProjectEntry[];
  diagnostics: LanguageProblem[];
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

export type DocumentViewId = "text" | "layout";

export type DocumentObjectDescriptor = {
  key: string;
  kind: string;
};

export type DocumentDescriptor = {
  path: string;
  objects: DocumentObjectDescriptor[];
  availableViews: DocumentViewId[];
  defaultObjectKeys: Partial<Record<DocumentViewId, string>>;
};

export type DistanceUnit = "meters" | "feet";

export type Point3 = {
  x: number;
  y: number;
  z: number;
};

export type Transform = {
  position: Point3;
  rotation: Point3;
  scale: Point3;
};

export type FixtureCatalogItem = {
  objectKey: string;
  sourcePath: string;
  importString: string;
  displayName: string;
  colorModel: string;
  geometry: string;
};

export type LayoutFixtureRef =
  | {
      type: "import";
      import: string;
      objectKey?: string | null;
      sourcePath?: string | null;
    }
  | {
      type: "inline";
      name: string;
      colorModel: string;
      geometry: unknown;
    };

export type LayoutFixturePlacement = {
  id: string;
  fixture: LayoutFixtureRef;
  transform: Transform;
  displayName?: string | null;
  colorModel?: string | null;
  geometry?: string | null;
};

export type LayoutGroupDocument = {
  name: string;
  members: string[];
};

export type LayoutDocument = {
  path: string;
  objectKey: string;
  name: string;
  units: DistanceUnit;
  fixtures: LayoutFixturePlacement[];
  groups: LayoutGroupDocument[];
  fixtureCatalog: FixtureCatalogItem[];
};

export type LayoutSaveResult = {
  serializedContent: string;
  project: ProjectState;
  analysis: AnalysisState;
};
