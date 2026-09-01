# CLAUDE.md

Project-level guidance for `/home/fenrir`, a single-machine **home-directory dotfiles repo** (working tree = `$HOME`).

## Repo shape

- Working tree is the entire home directory. Multiple independent git repos live under `$HOME` (`.claude/`, `.tmux/`, `.emacs.d/`, `.config/{alacritty,autostart,environment.d,fcitx5,keyd,systemd}`, `fenrir-tools/`, plus unrelated clones in `code/`, `WebstormProjects/`, `Documents/`). **None are submodules** — they are standalone clones hidden by `showUntrackedFiles=no`. [`GITS.org`](GITS.org) is the authoritative list of related repos (remotes, paths, build instructions).
- **Strategy: `status.showUntrackedFiles=no` + slim deny-then-whitelist `.gitignore`** (rebuilt 2026-05-09, root commit `e5d70b7`). The repo's history before that commit was discarded — only one user, no collaborators, so the rebuild was free.
  - **`$HOME` root noise** (caches, creds, IDE state, language toolchains, nested project repos, app state) is hidden by `git config status.showUntrackedFiles no`, NOT by `.gitignore` rules. The previous `$HOME`-root deny list was unmaintainable: `git status -uall` shows ~1M untracked items, every new app drops a new dir to chase. The config-based hide makes that whole problem disappear.
  - **`.local/`, `.gemini/`**: deny-then-whitelist. Even with untracked-hidden, these dirs need positive containment so `git add <dir>` doesn't sweep in `oauth_creds.json`-style siblings. Explicit `!`-rule per file/subdir we want. **`.config/`** is plain deny (`/.config/*`, no whitelist) — per-app config repos live there as independent clones (see [`GITS.org`](GITS.org)); the deny rule prevents accidental sweeps.
  - **`.ssh/`** stays untracked because `showUntrackedFiles=no` hides it — there's no longer an explicit `/.ssh/` rule in `.gitignore`. `~/.ssh/config` still has ~25 plaintext password comments + production hostnames across multiple work clients; the deferred follow-up is to split it via `Include ~/.ssh/config.local` (sanitized main file tracked, sensitive lines in untracked `.local`). Until that split happens, `.ssh/` remains opaque to the repo.
  - **To inspect what's hidden**: `git status -uall` is the one-shot opt-in. Don't make it the default — it dumps a million entries.
- For nested whitelist patterns under `.config/` etc., open **each level** (`!/.foo/`, `!/.foo/bar/`, `!/.foo/bar/**`) — git won't re-include children of an ignored parent even with `**`. See the `.gemini/` block in [`.gitignore`](.gitignore).
- To allow a single child of an otherwise-ignored directory, exclude the **contents** with `dir/*` (not the directory itself with `dir/`), then add `!dir/child`. The trailing `/` form excludes the directory entry and git stops walking, so child whitelists silently lose. The `.gemini/bin/` and `.local/bin/` blocks use `/*` for this reason.

## `.tmux/`: tmux config repo

`.tmux/` is an independent git repo (remote: `FenrirZheng/config-tmux`; see [`GITS.org`](GITS.org)). The parent dotfiles repo does NOT track it. `~/.tmux.conf` is a symlink to [`.tmux/tmux.conf`](.tmux/tmux.conf), hidden by `showUntrackedFiles=no`.

Inside that repo, three kinds of content:

- **TPM-managed plugins (untracked)** — `tpm`, `tmux-sensible`, `tmux-thumbs`, `tokyo-night-tmux`. Declared with `set -g @plugin` in [`tmux.conf`](.tmux/tmux.conf) and excluded **one line per plugin** in [`.tmux/.gitignore`](.tmux/.gitignore) (there's no `plugins/*` blanket — the in-repo plugin below has to stay visible). TPM re-clones them on `prefix + I`; local edits are throwaway, `prefix + U` overwrites them.
- **In-repo plugin (tracked as ordinary files)** — [`plugins/tmux-ace-window/`](.tmux/plugins/tmux-ace-window), the user's own bash ace-window port (`prefix + o` select / `prefix + O` swap). Not TPM-managed: `tmux.conf` loads it with an explicit `run-shell`, because its `@ace-window-*` options must be set before the loader runs.
- **Toolset (Rust)** — [`tools/`](.tmux/tools) is a tracked cargo workspace (`cc-attend`, `cc-beacon`, `cc-fleet`, `cc-launch`, `cc-layout`, `cc-tape`, `seek`, `sift`, `talk-fleet`, `to-claude`, `to-emacs`, `tmuxlib`). **This entry previously recorded `sift` as C++, built separately with cmake, not cargo (user directive, 2026-08-27).** On 2026-08-31, with the user's explicit approval, `sift` was ported to Rust and folded into the workspace — that directive is reversed, not silently forgotten; see [`docs/adr/0006-port-sift-from-cpp-to-rust.org`](.tmux/docs/adr/0006-port-sift-from-cpp-to-rust.org) for why (dialect fidelity with tmux's own POSIX-ERE search kept the regex engine on `libc` FFI rather than moving to the `regex` crate). [`claude.conf`](.tmux/claude.conf) binds keys straight at `tools/target/release/<bin>`, so those keys are dead until the build has run:

  ```bash
  cd ~/.tmux/tools
  cargo build --release
  ```

  Read [`tools/ARCHITECTURE.org`](.tmux/tools/ARCHITECTURE.org) before editing any crate; it has a per-tool atlas at [`tools/atlas/`](.tmux/tools/atlas) to read first.

Operational consequences:
- **Reload after editing config**: `tmux source-file ~/.tmux.conf` — re-runs TPM plus the explicit `run-shell` loaders (thumbs, ace-window, `claude.conf`).
- **Adding an upstream plugin**: add the `@plugin` line, `prefix + I`, **then add its directory to [`.tmux/.gitignore`](.tmux/.gitignore)** — the exclusions are per-plugin, so a newly cloned one shows up as untracked in `git -C ~/.tmux status` until you list it.
- **Fresh machine**: clone the tmux config repo to `~/.tmux/`, then `ln -sfn ~/.tmux/tmux.conf ~/.tmux.conf`, `prefix + I`, and run the build command above in `~/.tmux/tools` — skipping it leaves `prefix Space` and `prefix /` bound to "not built" stubs. See [`GITS.org`](GITS.org) for the remote URL.
- tmux-side documentation lives in that repo — [`README.md`](.tmux/README.md), [`runbooks/`](.tmux/runbooks), [`docs/adr/`](.tmux/docs/adr), [`plans/`](.tmux/plans).

## fenrir-tools/: locally-developed CLI tools

`fenrir-tools/` holds the user's own ACP helper CLIs as independent git repos (see [`GITS.org`](GITS.org) for remotes and build instructions). The parent dotfiles repo does NOT track them.

`~/.local/bin/{claud-chat,claude-chat}` are symlinks into these checkouts' build outputs. The symlinks are **not tracked** (`.local/bin/` is deny-then-whitelist and they aren't whitelisted) — recreate them after a fresh clone + build:

```bash
ln -sfn ~/fenrir-tools/claud-chat-acp/target/release/claud-chat  ~/.local/bin/claud-chat
ln -sfn ~/fenrir-tools/claude-agentic-chat/dist/index.js         ~/.local/bin/claude-chat
```

Each inner repo's own `.gitignore` handles its build artefacts — don't duplicate those in the root [`.gitignore`](.gitignore). Some carry their own `CLAUDE.md` — that's the place for tool-internal guidance, not this file.

## `.claude/`: Claude Code config + skills

`.claude/` (i.e. `~/.claude/`) is an independent git repo (remote: `FenrirZheng/claude-for-fenrir`; see [`GITS.org`](GITS.org)). The parent dotfiles repo does NOT track it.

Tool/skill-internal guidance belongs in `.claude/`'s own docs (its `CLAUDE.md`, per-skill `SKILL.md`), not this file. **Hooks**: inventoried in the [Active hooks section of global `CLAUDE.md`](.claude/CLAUDE.md#active-hooks-claudehooks); wired in [`settings.json`](.claude/settings.json).

## Keyboard remapping (keyd)

Physical-key remapping is done by [`keyd`](https://github.com/rvaiya/keyd) (system service, runs as root). keyd 2.5+ reads **only `/etc/keyd/*.conf`** — it does not look at `~/.config/keyd/`. So the keyd config repo at `~/.config/keyd/` carries `default.conf` as the **source of truth**; `/etc/keyd/default.conf` is a deployed copy. Deploy / re-deploy:

```bash
sudo cp ~/.config/keyd/default.conf /etc/keyd/default.conf && sudo systemctl restart keyd
```

If you ever edit `/etc/keyd/default.conf` in place (e.g. quick experiment), **copy it back** to `~/.config/keyd/default.conf` and commit, or the two drift. Rollback: `sudo systemctl stop keyd` (keyboard returns to stock behaviour), or restore one of the `default.conf.bak.*` siblings that live next to the deployed file in `/etc/keyd/`.

Current physical → logical map (laptop AT keyboard + 2 Logitech wireless; `[ids] *` covers all):

| physical key | behaviour | why |
|---|---|---|
| CapsLock | plain LeftCtrl | single-function — no tap/hold ambiguity |
| Right Alt | IME toggle (`command(/usr/local/bin/fcitx5-toggle)`) | dedicated single-function key; `us` layout doesn't use AltGr |
| Left Ctrl (bottom-left) | Super (`layer(meta)`) | corner key = Windows/Super, like a Mac; `layer(meta)` not bare `leftmeta` so it works as a real modifier in chords (keyd warns against bare meta on RHS) |
| Left Win | tap = Super (GNOME Activities), hold = `opt` layer (word-jump: `C-left/right/backspace`) | |
| Right Ctrl, Left Alt | unchanged | |

The IME toggle calls `/usr/local/bin/fcitx5-toggle` (root → drops to user `fenrir`'s session bus → `fcitx5-remote -t`). That script is **not tracked** (it's in `/usr/local/bin/`); recreate it on a fresh machine:

```bash
sudo tee /usr/local/bin/fcitx5-toggle >/dev/null <<'EOF'
#!/bin/bash
# Toggle fcitx5 IM state. Called by keyd (RightAlt) — keyd runs as root, so drop
# to user fenrir's session bus to reach the fenrir-owned fcitx5 daemon.
exec /usr/sbin/runuser -u fenrir -- env \
  DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus \
  XDG_RUNTIME_DIR=/run/user/1000 \
  DISPLAY=:0 \
  /usr/bin/fcitx5-remote -t
EOF
sudo chmod +x /usr/local/bin/fcitx5-toggle
```

`fcitx5` itself: [`/.config/fcitx5/config`](.config/fcitx5/config) keeps `TriggerKeys=F13` as a vestigial no-op (nothing emits F13 here) — the real toggle is the keyd→RightAlt→DBus path above; the `[Hotkey]` comment in that file says so.

## Commit conventions

`git log --oneline` shows a preference for **small, system-scoped commits that bundle config + service** together (e.g. `ab99e2b` adds zoxide-seed shell config and the emacs daemon systemd unit in one commit). Don't split a feature's client and service halves into separate commits unless they truly are independent.

Subject line style is loose: `<area>: <verb> <thing>` for substantive commits (`tmux: add TPM plugin scaffolding`), free-form (`goood`, `sh tool`) for trivial ones. Match the surrounding style of the area you're touching.

## Git pre-commit guard

> ⚠️ **NOT CURRENTLY WIRED — this guard does not run.** Measured 2026-08-18:
> `git -C ~ config --get core.hooksPath` returns **`/home/fenrir/.git/hooks`**,
> not `.githooks`, and that directory holds only `*.sample`. So no pre-commit
> check runs on this clone today, and nothing below is in force until the wiring
> command is re-run. The rest of this section describes the hook's *design*, kept
> because the file is still tracked and still works if re-enabled.
>
> **A hook's failure mode is silence** — same shape as the `$CLAUDE_TOOL_INPUT`
> incident recorded in [`.claude/CLAUDE.md`](.claude/CLAUDE.md#active-hooks-claudehooks),
> where two hooks blocked nothing for months while a table claimed they did. A
> document asserting a guard is active is not evidence that it is; only reading
> the config is.

Tracked at [`.githooks/pre-commit`](.githooks/pre-commit), wired per-clone via `git config core.hooksPath .githooks`. After a fresh clone you must re-run that command — `core.hooksPath` is a local config, not tracked in `.git/config` outside the clone. **That is exactly how it came to be un-wired here**: the setting is per-clone and nothing re-applies it.

The hook resolves its scanner as `$repo_root/.local/bin/gitleaks`, i.e. *relative to the repo it runs in*, and `exit 1`s if that path is not executable. It therefore only works in this `$HOME` clone — pointing another repo's `core.hooksPath` at it would not scan anything, it would **block every commit there** with a "not found" failure.

One check, fatal:

- **`gitleaks git --staged`** (binary at `~/.local/bin/gitleaks`, installed manually from upstream releases — not tracked) — content-based scan for credential patterns (OAuth tokens, AWS keys, private keys, GitHub PATs). With `showUntrackedFiles=no` hiding most of `$HOME`, the main risk shifts to "I deliberately `git add`ed a file that contains a token I forgot about" — gitleaks is the last-mile defense against that.

Coverage caveat: gitleaks regex catches **high-confidence patterns** like `password: 6Qr...`, `AKIA...`, `ghp_...`. It does NOT catch free-form password comments (`# pwd: foo`, `## user x/y`). Don't rely on gitleaks alone — keep secrets out of tracked files entirely. The original audit of `.ssh/config` found ~20 such free-form leaks; that file remains untracked (now via `showUntrackedFiles=no` rather than an explicit ignore rule) pending the `Include ~/.ssh/config.local` split.

If the hook blocks a commit: read the failure mode and fix the underlying issue. Do not reach for `--no-verify`.

## Fresh-clone bootstrap

After cloning into `$HOME` on a new machine:

1. `git -C ~ config status.showUntrackedFiles no` — hide the ~1M `$HOME` items the repo doesn't track. Without this, `git status` is unusable.
2. `git -C ~ config core.hooksPath .githooks` — wire up pre-commit (`core.hooksPath` is local config, not tracked). **Optional, and currently NOT applied on this machine** — see the [pre-commit guard section](#git-pre-commit-guard). Skip it deliberately if you don't want the gitleaks gate; just don't leave the docs claiming it's on.
3. Clone all related repos listed in [`GITS.org`](GITS.org) to their expected paths (`.claude/`, `.tmux/`, `.emacs.d/`, `.config/{alacritty,autostart,environment.d,fcitx5,keyd,systemd}`, `fenrir-tools/{claud-chat-acp,claude-agentic-chat}`).
4. tmux: `ln -sfn ~/.tmux/tmux.conf ~/.tmux.conf`, `prefix + I`, then `cd ~/.tmux/tools && cargo build --release` — see the [tmux repo section](#tmux-tmux-config-repo). **This step used to require a second build**: a C++20 compiler (g++ ≥ 10) and `cmake`, to build `sift` separately (user directive, 2026-08-27). That directive was reversed 2026-08-31, with the user's approval — `sift` was ported to Rust and folded into the same `cargo build --release`, so those two dependencies are gone and there is only one command; see [`docs/adr/0006-port-sift-from-cpp-to-rust.org`](.tmux/docs/adr/0006-port-sift-from-cpp-to-rust.org).
5. fenrir-tools: build both CLIs, recreate symlinks — see [fenrir-tools section](#fenrir-tools-locally-developed-cli-tools).
6. Install gitleaks to `~/.local/bin/gitleaks` from [upstream releases](https://github.com/gitleaks/gitleaks/releases) (binary, not tracked) — required **only if** you wired the hook in step 2; it is also useful on its own for a manual `gitleaks git --staged` run.
7. Keyboard remap: install `keyd`, recreate `/usr/local/bin/fcitx5-toggle` and deploy the keyd config — `sudo cp ~/.config/keyd/default.conf /etc/keyd/default.conf && sudo systemctl enable --now keyd`. See the [Keyboard remapping section](#keyboard-remapping-keyd) for the `fcitx5-toggle` script body.
8. Terminal font: install **DejaVuSansM Nerd Font** (font files, not tracked anywhere — same class of manual dependency as gitleaks). `~/.config/alacritty/alacritty.toml` names the plain variant (`DejaVuSansM Nerd Font`, deliberately NOT `…Mono` — Mono shrinks icons into one cell), and without the font the tmux status bar's powerline caps and PUA icons degrade to overlapping tofu (the 2026-08-24 incident that motivated this step):

   ```bash
   mkdir -p ~/.local/share/fonts/DejaVuSansMNerdFont
   curl -sL -o /tmp/DejaVuSansMono.zip \
     https://github.com/ryanoasis/nerd-fonts/releases/latest/download/DejaVuSansMono.zip
   unzip -oq /tmp/DejaVuSansMono.zip -d ~/.local/share/fonts/DejaVuSansMNerdFont 'DejaVuSansMNerdFont*.ttf'
   fc-cache -f ~/.local/share/fonts
   ```

   (Release asset is named `DejaVuSansMono.zip`; the family inside is `DejaVuSansM Nerd Font`.)

Verify with `git -C ~ status` (should be clean, with the `(use -u to show untracked files)` hint).

If you chose to wire the hook in step 2, verify **that it actually runs** — an
empty commit succeeds identically whether the hook fires or not, so on its own it
proves nothing:

```bash
git -C ~ config --get core.hooksPath          # must print .githooks
git -C ~ commit --allow-empty -m test 2>&1 | grep 'pre-commit: gitleaks'
git -C ~ reset --soft HEAD~1
```

Absence of that `pre-commit: gitleaks` line means the guard is not in the path,
whatever the config says.

## Don't

- **Never `git add .` or `git add -A` at `$HOME`.** Under `showUntrackedFiles=no` it's tempting because `git status` looks clean, but `add .` ignores that config — it walks the actual filesystem and would try to stage everything not gitignored. The slim `.gitignore` only denies app-state regions and build artefacts; vast tracts of `$HOME` (caches, creds, history files, downloads) are NOT in the deny list — they were untracked-by-config, not untracked-by-rule. `git add <specific-path>` always; never the cwd shortcut.
- Don't repopulate the old `$HOME`-root deny list in `.gitignore`. It was deliberately deleted in the `e5d70b7` rebuild — the config-based hide replaces it. If you find yourself wanting to add `/.someapp/` to ignore-noise, the answer is "it's already hidden, you're looking at `git status -uall` output".
- Don't restore `/.config/fcitx5/conf/cached_layouts` to tracked. fcitx5 rewrites it on every run; the rebuild intentionally dropped it. If `git status -uall` shows it as untracked, that's correct — let fcitx5 own it.
- Don't edit `/etc/keyd/default.conf` and forget to mirror it back to `~/.config/keyd/default.conf` — the config repo copy is the source of truth, the `/etc/` one is a deploy target. See the [Keyboard remapping section](#keyboard-remapping-keyd).
- Don't add submodule tracking back. All related repos are standalone clones listed in [`GITS.org`](GITS.org) — the parent dotfiles repo deliberately does not track them.
