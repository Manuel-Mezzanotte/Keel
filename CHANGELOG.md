# Changelog

What changes in each version of Keel, the latest first. Keel updates
itself: the notes it shows in the update come from here, shorter.

*In italiano: [docs/CHANGELOG.it.md](docs/CHANGELOG.it.md).*

## 0.1.3 — 27 November 2026

### Claude

- **Chats of other projects open here,** in the window you work in, with
  Claude working in their folder: a strip over the chat says where, with
  "Open the project". "+" comes back to this project.
- **Write while Claude works:** a message waits over the box and goes as
  soon as the turn ends; several go as one. Esc in the box stops Claude.
- **The open chat comes back** when you open the project again.
- **Bring the code back to a message:** on one of your messages, every
  file changed from that turn on goes back as it was; a file Claude made
  goes to the Trash. From an answer, **go on in a new chat** from there.
- **The plan in a tab:** when Claude asks "Ready to code?", its plan opens
  in the editor, to read whole, change and annotate; "Revise with my
  notes" sends it back.
- **Comments on Claude's changes:** in a change's tab, a line's number or a
  block's 💬 opens a comment; the comments show under their lines and go
  to Claude together, each with its code.
- **Dictation** in the message box: the microphone, or ⌘D.
- **Rename a chat, copy an answer,** and "Add to Claude" with a right click
  on files of the tree.
- **Model names you can read** ("Opus 5.5"), and the Mac stays awake while
  Claude works (Settings › Files and Windows).

### Icons

- **Material Icon Theme built in:** files by name and extension (`.tsx`
  with React's atom, `package.json`, `Dockerfile`…) and folders open or
  closed, from the first start, in the tree, tabs, Git, Search, Quick Open,
  Problems and Claude's panel. The plain symbols stay as "Simple icons".
- **Projects with their own picture** (their icon, favicon or logo) in the
  recent projects and in Claude's chats.

### The page beside the code

- **A web page in a tab:** the project's page (`localhost:3000`) opens in
  Keel, from Ports or the palette, with back, forward, reload, a phone's
  and a tablet's width, Safari's inspector, "Open in the browser" and
  "Show to Claude", which puts a picture of the page in the next message.
  It is macOS's WebKit: its processes exist only while a page is open.

### Git

- **What the branch changes** against `main`, in one page, before opening a
  pull request.
- **The commit message in a big window:** Shift+Esc in the message, or ⤢
  under it; ⌘↩ commits from there.
- **Files dragged from Source Control** into the terminal (their path), the
  editor (they open) or Claude's chat.

### Smaller things

- **Quick Open:** → in an empty field, or ⌥↩, opens a file behind the list
  and keeps the list open.
- **"Don't Show Again"** on the notices that come back (extensions,
  programs in the terminal, long commands, Claude); a command brings them
  back.
- **⌘ with the wheel** makes the terminal's text bigger or smaller.

### Fixed

- The tests of a crate in a subfolder of a project without a Cargo
  workspace did not start.

## 0.1.2 — 25 November 2026

### Claude's panel, redone

- **Sign in and out from the panel.** Without a login, the panel offers a
  Claude subscription (Pro, Max, Team, Enterprise) or the Anthropic
  Console: Claude Code's own login runs in a terminal and the browser
  opens; the chat starts as soon as it lands. The account menu shows the
  plan and the e-mail, and logs out.
- **An API key of your own,** kept in the macOS keychain, never in a file,
  and given to Claude Code when it starts. It can be forgotten again.
- **Model, reasoning and mode in cards** over the message box: the main
  models first and the older ones under "More models"; the reasoning on a
  slider to drag, with a name for each level and Auto as a switch; fast
  mode; the mode (Manual, Accept edits, Plan, Auto, Bypass permissions)
  with a line on what each one does.
- **`/` for commands and skills:** at the start of a message, Claude Code's
  commands and your own skills, filtered as you type.
- **`@` for files and folders:** anywhere in the message, the selection,
  the open files and any file or folder of the project, by fuzzy search.
- **Context and plan limits:** a ring in the message box shows how full the
  chat's context is; its card shows the tokens, the 5-hour and weekly
  limits of your plan with when they reset, read with Claude Code's own
  `/usage` without spending tokens. With an API key, the chat's cost.
- **Chats shared with Claude Code and the Claude app:** a chat continued in
  the terminal, or anywhere else, is read again when you come back to it.
  The Claude app's chats show under their title, one card for each
  project, the open project first; a chat of another project opens in that
  project's window.
- **Delete a chat** from its bin, with a second click to confirm.
- **New look:** Claude's mark, a home with things to ask, your messages in
  bubbles, tools as quiet rows (commands of several lines no longer spill
  over the rows below, and opened rows no longer vanish in a scrolled
  chat), reasoning on one line that opens readable, the plan with its
  progress, permissions in their own card.

### The first minutes

- **Import from VS Code**, as from Cursor: settings, shortcuts, snippets,
  recent projects and extensions. Nothing of Keel's is overwritten.
- **"Set up Keel"** after the first start's welcome: the theme, import
  from the editors found on the Mac, the `keel` command.
- **The `keel` command in the Terminal:** `keel .` opens the folder in Keel
  and gives the prompt back at once, as `code .` does.

### Fixed

- Keel at rest, with a file open, used about 7% of a core and kept its
  graphics memory: now it uses none, and memory went from 272 to 157 MB.
  The caret stops blinking after 10 seconds without typing.

## 0.1.1 — 24 November 2026

- **Settings redrawn:** each page with its icon, one search over every
  setting (name, description and key), cards by group, a mark on what you
  changed and a button back to Keel's value.
- **Keyboard shortcuts** with a search, keys drawn as keys and the buttons
  only on the row under the pointer.
- **Welcome page** with Keel's mark, the actions and the recent projects in
  two columns, and a calm sea at the bottom.
- **A welcome for the first start:** sky, sea and Keel's boat, then "Come
  aboard". Help › Show the Welcome Again plays it again.
- The notices of the open source software built into Keel travel with it,
  in Settings › Updates.

## 0.1.0 — 23 November 2026

The first public version: editor, Git with its graph, terminal, debugger,
tests, pull requests and Claude Code in a single app for the Mac. From here
on, updates come from Keel itself.
