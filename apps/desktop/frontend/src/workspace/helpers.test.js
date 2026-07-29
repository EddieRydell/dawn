import assert from "node:assert/strict";
import test from "node:test";

import {
  buildSemanticTree,
  locationRange,
  matchesCommand,
  rankQuickOpenFiles,
  remapWorkspacePath,
  sameWorkspacePath
} from "./helpers.ts";

const entry = (path, kind, role = "file") => ({
  path,
  kind,
  name: path.split("/").pop(),
  parent: path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "",
  role,
  ownership: "project",
  operations: ["open"],
  operationExplanation: null
});

test("semantic tree sorts directories first with natural case-insensitive ordering", () => {
  const tree = buildSemanticTree([
    entry("file10.dawn", "file"),
    entry("Folder", "directory", "directory"),
    entry("file2.dawn", "file"),
    entry("Folder/z.dawn", "file")
  ], [], "C:/project");
  assert.deepEqual(tree.map((node) => node.name), ["Folder", "file2.dawn", "file10.dawn"]);
  assert.equal(tree[0].children[0].path, "Folder/z.dawn");
});

test("diagnostic paths resolve from absolute project paths", () => {
  const tree = buildSemanticTree(
    [entry("sequences/show.sequence.dawn", "file", "sequence")],
    [{
      path: "C:\\project\\sequences\\show.sequence.dawn",
      range: null,
      severity: "error",
      code: "test",
      message: "broken"
    }],
    "C:\\project"
  );
  assert.equal(tree[0].errorCount, 1);
  assert.equal(sameWorkspacePath("C:\\project\\a.dawn", "a.dawn", "C:\\project"), true);
});

test("quick open ranks open files then recent files then the remaining project files", () => {
  const snapshot = {
    tabs: [{ path: "open.dawn" }],
    workspaceExplorer: { recentFiles: ["recent.dawn", "open.dawn"] },
    projectEntries: [
      entry("other.dawn", "file"),
      entry("open.dawn", "file"),
      entry("recent.dawn", "file")
    ]
  };
  assert.deepEqual(rankQuickOpenFiles(snapshot), ["open.dawn", "recent.dawn", "other.dawn"]);
});

test("command filtering, path remapping, and navigation ranges are deterministic", () => {
  assert.equal(matchesCommand("Focus Problems", "View", ["errors", "sidebar"], "view error"), true);
  assert.equal(matchesCommand("Focus Problems", "View", ["errors"], "package"), false);
  assert.equal(remapWorkspacePath("effects/a.dawn", "effects", "library/effects"), "library/effects/a.dawn");
  assert.deepEqual(locationRange(3, 4, 5), {
    start: { line: 3, character: 4 },
    end: { line: 3, character: 9 }
  });
});
