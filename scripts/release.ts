#!/usr/bin/env bun
// Versions, tags and release notes. No dependencies beyond Bun and git.
//
//   bun run bump patch|minor|major|1.2.3 [--push] [--dry-run]
//       Sets the version in Cargo.toml and Cargo.lock, commits "Release v1.2.3", tags v1.2.3 and
//       shows the release notes the tag will get. Pushing the tag (--push, or by hand) starts
//       .github/workflows/release.yml.
//   bun scripts/release.ts nightly-version --run 42
//       The version CI gives a preview build: the next patch after the last release, then
//       -nightly.<date>.<run>, so 0.2.1-nightly.20261115.42 sorts after 0.2.0 and before 0.2.1.
//   bun scripts/release.ts notes --tag v1.2.3 [--ref HEAD] [--repo owner/name] [--output notes.md]
//       Release notes in the same shape as T3 Code's: a header with the counts, the changes that
//       matter as bullets, a count of the rest, and every commit folded under "Full changelog".
//       notes/<tag>.md, if it exists, replaces the bullets with your own words (a first release,
//       or one whose commits don't tell the story).

import { existsSync, readFileSync, writeFileSync } from "node:fs";

const ROOT = new URL("..", import.meta.url).pathname;
const MAX_HIGHLIGHTS = 7;
const KEEP_NIGHTLIES = 10;

// ── Versions ────────────────────────────────────────────────────────────────────────────────

export interface Version {
  major: number;
  minor: number;
  patch: number;
  pre: string[];
}

export function parseVersion(text: string): Version | undefined {
  const match = /^v?(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+.*)?$/.exec(text.trim());
  if (!match) return undefined;
  const [, major, minor, patch, pre] = match;
  return { major: Number(major), minor: Number(minor), patch: Number(patch), pre: pre ? pre.split(".") : [] };
}

export const formatVersion = (v: Version) => `${v.major}.${v.minor}.${v.patch}${v.pre.length ? `-${v.pre.join(".")}` : ""}`;

/** Semantic versioning's order; the same as `compare` in src/update.rs. */
export function compareVersions(a: Version, b: Version): number {
  const core = a.major - b.major || a.minor - b.minor || a.patch - b.patch;
  if (core) return Math.sign(core);
  if (!a.pre.length || !b.pre.length) return Math.sign(b.pre.length - a.pre.length);
  for (let i = 0; i < Math.max(a.pre.length, b.pre.length); i++) {
    const [p, q] = [a.pre[i], b.pre[i]];
    if (p === undefined) return -1;
    if (q === undefined) return 1;
    const [m, n] = [/^\d+$/.test(p), /^\d+$/.test(q)];
    if (m && n && Number(p) !== Number(q)) return Math.sign(Number(p) - Number(q));
    if (m !== n) return m ? -1 : 1;
    if (!m && p !== q) return p < q ? -1 : 1;
  }
  return 0;
}

export function bumpVersion(current: Version, how: string): Version {
  const { major, minor, patch } = current;
  switch (how) {
    case "major":
      return { major: major + 1, minor: 0, patch: 0, pre: [] };
    case "minor":
      return { major, minor: minor + 1, patch: 0, pre: [] };
    case "patch":
      return { major, minor, patch: patch + 1, pre: [] };
    default: {
      const exact = parseVersion(how);
      if (!exact) throw new Error(`"${how}" isn't patch, minor, major or a version like 1.2.3`);
      return exact;
    }
  }
}

/** The kind of tag: a release (v1.2.3), a preview (v1.2.4-nightly.20261115.42), or neither. */
export function channelOf(tag: string): "stable" | "nightly" | undefined {
  const v = parseVersion(tag);
  if (!v || !tag.startsWith("v")) return undefined;
  if (!v.pre.length) return "stable";
  return v.pre[0] === "nightly" ? "nightly" : undefined;
}

/** The tag notes are counted from: the newest earlier tag of the same kind. A first nightly
 * counts from the last release. */
export function previousTag(tag: string, tags: string[]): string | undefined {
  const current = parseVersion(tag);
  const channel = channelOf(tag);
  if (!current || !channel) throw new Error(`${tag} isn't a release or nightly tag`);
  const newest = (kind: string) =>
    tags
      .filter((t) => t !== tag && channelOf(t) === kind)
      .map((t) => ({ t, v: parseVersion(t)! }))
      .filter(({ v }) => compareVersions(v, current) < 0)
      .sort((a, b) => compareVersions(b.v, a.v))[0]?.t;
  return newest(channel) ?? (channel === "nightly" ? newest("stable") : undefined);
}

export function nightlyVersion(cargo: Version, tags: string[], date: Date, run: number): string {
  const released = tags.includes(`v${formatVersion({ ...cargo, pre: [] })}`);
  const base = released ? bumpVersion(cargo, "patch") : { ...cargo, pre: [] };
  const day = date.toISOString().slice(0, 10).replaceAll("-", "");
  return formatVersion({ ...base, pre: ["nightly", day, String(run)] });
}

// ── Release notes ───────────────────────────────────────────────────────────────────────────

export interface Commit {
  sha: string;
  subject: string;
  body: string;
  author: string;
}

const SKIPPED = /^(chore|docs|test|tests|ci|build|style|refactor)(\([^)]*\))?!?:/i;
const FIXES = /^(fix|perf)(\([^)]*\))?!?:|^fix(es|ed)?\b/i;
const RELEASES = /^Release v\d/;
const BOTS = /\[bot\]$/;

/** Bump commits and merges say nothing about what changed. */
const counted = (c: Commit) => !RELEASES.test(c.subject) && !/^Merge (pull request|branch)/.test(c.subject);

/** A subject without its conventional-commit prefix, capitalised: "feat(ui): add x" → "Add x". */
function label(subject: string): string {
  const bare = subject.replace(/^[a-z]+(\([^)]*\))?!?:\s*/i, "").replace(/\s*\(#\d+\)$/, "").replace(/\.$/, "");
  return bare.charAt(0).toUpperCase() + bare.slice(1);
}

/** The first sentence of a commit's prose, if it opens with prose rather than a list. */
function detail(body: string): string | undefined {
  const first = body.split(/\n\s*\n/)[0]?.replace(/\s+/g, " ").trim() ?? "";
  if (!first || /^[-*\d]|^Co-Authored-By|^Claude-Session/i.test(first)) return undefined;
  const sentence = /^.*?[.!?](?=\s|$)/.exec(first)?.[0] ?? first;
  return sentence.split(" ").length <= 30 ? sentence : undefined;
}

const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;

export function releaseNotes(o: {
  version: string;
  commits: Commit[];
  repo?: string;
  previous?: string;
  tag: string;
  nightlyOf?: string;
  /** Hand-written bullets (notes/<tag>.md), used in place of the ones made from commits. */
  written?: string;
}): string {
  const commits = o.commits.filter(counted);
  const people = new Set(commits.map((c) => c.author).filter((a) => !BOTS.test(a)).map((a) => a.toLowerCase()));
  const header = `**v${o.version}** · ${plural(commits.length, "change")} from ${plural(people.size, "contributor")}`;

  const features = commits.filter((c) => !SKIPPED.test(c.subject) && !FIXES.test(c.subject));
  const fixes = commits.filter((c) => FIXES.test(c.subject));
  const shown = features.slice(0, MAX_HIGHLIGHTS);
  const bullets = shown.map((c) => {
    const more = detail(c.body);
    return more ? `- **${label(c.subject)}.** ${more}` : `- **${label(c.subject)}.**`;
  });
  // Like T3 Code: what didn't get a bullet is counted, not listed.
  const rest = features.length - shown.length;
  const fixWord = fixes.length === 1 ? "bug fix" : "bug fixes";
  let summary = bullets.join("\n");
  if (o.written?.trim()) {
    summary = o.written.trim();
  } else if (bullets.length && (rest || fixes.length)) {
    const parts = [];
    if (rest) parts.push(`${rest} more ${rest === 1 ? "change" : "changes"}`);
    if (fixes.length) parts.push(`${fixes.length}${rest ? "" : " more"} ${fixWord}`);
    summary += `\n\n### Plus ${parts.join(" and ")}.`;
  } else if (fixes.length) {
    summary = `**${fixes.length} ${fixWord}.**`;
  }

  const intro = o.nightlyOf
    ? `> Nightly build of \`${o.nightlyOf.slice(0, 7)}\`, made from every change to main before it's in a release. ` +
      `It can be rough. Turn on **Preview versions** in Tendril (search for "Appearance") to get these as they come.`
    : "";

  const list = o.commits
    .filter(counted)
    .map((c) => `- ${c.subject} (${c.sha.slice(0, 7)}) by ${c.author}`)
    .join("\n");
  const compare = o.repo && o.previous ? `\n\n**Full changelog**: https://github.com/${o.repo}/compare/${o.previous}...${o.tag}` : "";
  const folded = `<details>\n<summary>Full changelog</summary>\n\n${list || "No changes."}${compare}\n\n</details>\n`;
  return [header, intro, summary, folded].filter(Boolean).join("\n\n");
}

// ── git ─────────────────────────────────────────────────────────────────────────────────────

function git(...args: string[]): string {
  const result = Bun.spawnSync(["git", ...args], { cwd: ROOT, stderr: "pipe" });
  if (result.exitCode !== 0) throw new Error(`git ${args.join(" ")}: ${result.stderr.toString().trim()}`);
  return result.stdout.toString().trim();
}

const tags = () => git("tag", "--list").split("\n").filter(Boolean);

function commitsBetween(from: string | undefined, to: string): Commit[] {
  const range = from ? `${from}..${to}` : to;
  const raw = git("log", "--no-merges", "--format=%H%x1f%s%x1f%an%x1f%b%x1e", range);
  return raw
    .split("\x1e")
    .map((r) => r.trim())
    .filter(Boolean)
    .map((r) => {
      const [sha = "", subject = "", author = "", body = ""] = r.split("\x1f");
      return { sha, subject, author, body };
    });
}

function cargoVersion(): Version {
  const toml = readFileSync(`${ROOT}Cargo.toml`, "utf8");
  const text = /^\[package\][^[]*?^version\s*=\s*"([^"]+)"/m.exec(toml)?.[1];
  const version = text && parseVersion(text);
  if (!version) throw new Error("no version in Cargo.toml's [package]");
  return version;
}

function setCargoVersion(from: Version, to: Version) {
  const [old, next] = [formatVersion(from), formatVersion(to)];
  const tomlPath = `${ROOT}Cargo.toml`;
  const toml = readFileSync(tomlPath, "utf8");
  writeFileSync(tomlPath, toml.replace(/^(\[package\][^[]*?^version\s*=\s*)"[^"]+"/m, `$1"${next}"`));
  const lockPath = `${ROOT}Cargo.lock`;
  if (existsSync(lockPath)) {
    const lock = readFileSync(lockPath, "utf8");
    const entry = `name = "tendril"\nversion = "${old}"`;
    if (!lock.includes(entry)) throw new Error("Cargo.lock doesn't list tendril at the current version");
    writeFileSync(lockPath, lock.replace(entry, `name = "tendril"\nversion = "${next}"`));
  }
}

// ── Commands ────────────────────────────────────────────────────────────────────────────────

function flags(args: string[]): { positional: string[]; get: (name: string) => string | undefined; has: (name: string) => boolean } {
  const positional: string[] = [];
  const named = new Map<string, string>();
  for (let i = 0; i < args.length; i++) {
    const a = args[i]!;
    if (!a.startsWith("--")) positional.push(a);
    else if (args[i + 1] !== undefined && !args[i + 1]!.startsWith("--")) named.set(a.slice(2), args[++i]!);
    else named.set(a.slice(2), "");
  }
  return { positional, get: (n) => named.get(n), has: (n) => named.has(n) };
}

function notes(tag: string, ref: string, repo?: string): string {
  const version = parseVersion(tag);
  if (!version) throw new Error(`${tag} isn't a version tag`);
  const previous = previousTag(tag, tags());
  const written = `${ROOT}notes/${tag}.md`;
  return releaseNotes({
    written: existsSync(written) ? readFileSync(written, "utf8") : undefined,
    version: formatVersion(version),
    tag,
    previous,
    repo,
    commits: commitsBetween(previous, ref),
    nightlyOf: channelOf(tag) === "nightly" ? git("rev-parse", ref) : undefined,
  });
}

function bump(how: string | undefined, push: boolean, dry: boolean) {
  if (!how) throw new Error("say how: bun run bump patch, minor, major, or a version like 1.2.3");
  if (!dry && git("status", "--porcelain")) throw new Error("commit or stash your changes first");
  const branch = git("rev-parse", "--abbrev-ref", "HEAD");
  if (branch !== "main") console.warn(`Note: you're on ${branch}, not main.`);
  const current = cargoVersion();
  const next = bumpVersion(current, how);
  const tag = `v${formatVersion(next)}`;
  if (tags().includes(tag)) throw new Error(`${tag} already exists`);
  const same = compareVersions(next, current) === 0;
  if (!same && compareVersions(next, current) < 0) throw new Error(`${formatVersion(next)} is before ${formatVersion(current)}`);
  if (next.pre.length) throw new Error("previews come from CI; releases are plain versions like 1.2.3");

  console.log(`${formatVersion(current)} → ${formatVersion(next)}\n`);
  console.log(notes(tag, "HEAD", "devatloam/tendril"));
  if (dry) return console.log("\n(dry run: nothing changed)");

  if (!same) {
    setCargoVersion(current, next);
    git("commit", "--quiet", "--all", "--message", `Release ${tag}`);
  }
  git("tag", "--annotate", tag, "--message", `Tendril ${formatVersion(next)}`);
  if (push) {
    git("push", "--atomic", "origin", "HEAD", tag);
    console.log(`\nPushed ${tag}. The release workflow builds and publishes it.`);
  } else {
    console.log(
      `\nTagged ${tag}. Publish it with:\n  git push --atomic origin HEAD ${tag}\n` +
        `Or, where tags can't be pushed (cloud sessions): delete the tag (git tag -d ${tag}), push main, and run\n` +
        `  gh workflow run release.yml --ref main\n` +
        `which makes the tag on GitHub.`,
    );
  }
}

/** Which nightly releases to delete, keeping the newest few. */
export function staleNightlies(all: string[], keep = KEEP_NIGHTLIES): string[] {
  return all
    .filter((t) => channelOf(t) === "nightly")
    .sort((a, b) => compareVersions(parseVersion(b)!, parseVersion(a)!))
    .slice(keep);
}

if (import.meta.main) {
  const [command, ...rest] = Bun.argv.slice(2);
  const f = flags(rest);
  try {
    switch (command) {
      case "bump":
        bump(f.positional[0], f.has("push"), f.has("dry-run"));
        break;
      case "nightly-version":
        console.log(nightlyVersion(cargoVersion(), tags(), new Date(), Number(f.get("run") ?? 0)));
        break;
      case "notes": {
        const tag = f.get("tag");
        if (!tag) throw new Error("--tag is needed");
        const text = notes(tag, f.get("ref") ?? "HEAD", f.get("repo"));
        const out = f.get("output");
        if (out) writeFileSync(out, `${text}\n`);
        else console.log(text);
        break;
      }
      case "stale-nightlies":
        console.log(staleNightlies(f.positional.length ? f.positional : tags()).join("\n"));
        break;
      default:
        throw new Error(`unknown command "${command ?? ""}": bump, nightly-version, notes or stale-nightlies`);
    }
  } catch (e) {
    console.error(e instanceof Error ? e.message : e);
    process.exit(1);
  }
}
