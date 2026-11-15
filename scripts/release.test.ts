import { expect, test } from "bun:test";
import { bumpVersion, compareVersions, formatVersion, nightlyVersion, parseVersion, previousTag, releaseNotes, staleNightlies } from "./release";

const v = (text: string) => parseVersion(text)!;

test("versions sort like the updater sorts them", () => {
  const order = ["0.1.0", "0.1.1-nightly.20261114.9", "0.1.1-nightly.20261115.10", "0.1.1-nightly.20261115.11", "0.1.1", "0.2.0", "0.10.0"];
  for (let i = 1; i < order.length; i++) expect(compareVersions(v(order[i]!), v(order[i - 1]!))).toBe(1);
  expect(compareVersions(v("v1.2.3"), v("1.2.3"))).toBe(0);
});

test("bumps", () => {
  expect(formatVersion(bumpVersion(v("0.1.4"), "patch"))).toBe("0.1.5");
  expect(formatVersion(bumpVersion(v("0.1.4"), "minor"))).toBe("0.2.0");
  expect(formatVersion(bumpVersion(v("0.1.4"), "major"))).toBe("1.0.0");
  expect(formatVersion(bumpVersion(v("0.1.4"), "2.0.0"))).toBe("2.0.0");
  expect(() => bumpVersion(v("0.1.4"), "huge")).toThrow();
});

test("nightlies lead up to the next release", () => {
  const day = new Date("2026-11-15T08:00:00Z");
  expect(nightlyVersion(v("0.1.0"), [], day, 42)).toBe("0.1.0-nightly.20261115.42");
  expect(nightlyVersion(v("0.1.0"), ["v0.1.0"], day, 43)).toBe("0.1.1-nightly.20261115.43");
});

test("notes count from the last tag of the same kind", () => {
  const tags = ["v0.1.0", "v0.2.0", "v0.2.1-nightly.20261114.5", "v0.2.1-nightly.20261115.6", "update-feed"];
  expect(previousTag("v0.3.0", tags)).toBe("v0.2.0");
  expect(previousTag("v0.2.1-nightly.20261115.7", tags)).toBe("v0.2.1-nightly.20261115.6");
  expect(previousTag("v0.3.1-nightly.20261201.1", ["v0.3.0"])).toBe("v0.3.0");
  expect(previousTag("v0.1.0", [])).toBeUndefined();
});

test("old nightlies are pruned, newest kept", () => {
  const tags = Array.from({ length: 12 }, (_, i) => `v0.2.1-nightly.20261115.${i + 1}`).concat("v0.2.0");
  expect(staleNightlies(tags, 10)).toEqual(["v0.2.1-nightly.20261115.2", "v0.2.1-nightly.20261115.1"]);
});

test("release notes: header, highlights, the rest counted, everything folded", () => {
  const c = (subject: string, body = "", author = "Ada") => ({ sha: "abcdef1234", subject, body, author });
  const notes = releaseNotes({
    version: "0.2.0",
    tag: "v0.2.0",
    previous: "v0.1.0",
    repo: "devatloam/tendril",
    commits: [
      c("Clipboard history", "Copies are kept as you make them. Search them by typing.\n\n- more"),
      c("fix: the window opened off screen", "", "Bo"),
      c("docs: explain previews"),
      c("Release v0.1.1"),
      c("Window switcher", "- Lists open windows"),
    ],
  });
  expect(notes).toStartWith("**v0.2.0** · 4 changes from 2 contributors");
  expect(notes).toContain("- **Clipboard history.** Copies are kept as you make them.");
  expect(notes).toContain("- **Window switcher.**\n");
  expect(notes).toContain("### Plus 1 more bug fix.");
  expect(notes).toContain("<summary>Full changelog</summary>");
  expect(notes).toContain("https://github.com/devatloam/tendril/compare/v0.1.0...v0.2.0");
  expect(notes).not.toContain("Release v0.1.1");
});
