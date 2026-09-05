# chatTUI

chatTUI is a terminal chat client for OpenAI-compatible APIs, with a default
endpoint of [Dahl Inference](https://inference.dahl.global/). It is a single
Rust binary built with `ratatui`, `crossterm`, `tokio`, and `reqwest`.

The composer is always active. There are no Vim modes.

## Features

- Streaming responses over OpenAI-compatible SSE (`POST /chat/completions`)
- Markdown rendering with shaded fenced code blocks
- Local conversation history stored as JSON
- History overlay to switch or delete saved conversations
- Copy a fenced code block to the clipboard
- Slash commands and a keyboard-shortcuts overlay
- Unicode-aware composer (cursor, backspace, delete, word jumps, paste)
- Footer shows message count and an approximate token estimate

## Requirements

- Rust and Cargo from [rustup.rs](https://rustup.rs/)
- A Dahl API key from [Dahl Inference](https://inference.dahl.global/), or a
  key for another OpenAI-compatible endpoint

```bash
rustc --version
cargo --version
```

## Install

```bash
git clone https://github.com/AmirRanger149/chatTUI-dahl.git
cd chatTUI-dahl
cargo build --release
```

The binary is `target/release/chat-tui`. During development:

```bash
cargo run
```

## Configuration

Values are applied in this order:

1. Built-in defaults
2. `dahl.json`
3. Environment variables, which override the file

`dahl.json` is read from the directory that contains the compiled binary, then
from the current working directory. The first file found is used.

```json
{
  "api_key": "your-key",
  "base_url": "https://inference.dahl.global/v1",
  "model": "MiniMaxAI/MiniMax-M2.7",
  "temperature": 0.7
}
```

Every field is optional. Missing fields keep their defaults:

| Field | Default |
| --- | --- |
| `api_key` | unset |
| `base_url` | `https://inference.dahl.global/v1` |
| `model` | `MiniMaxAI/MiniMax-M2.7` |
| `temperature` | `0.7` |

Keep `dahl.json` private. It is gitignored and should not be committed.

### Environment variables

These override the matching values from `dahl.json` when they are set and
non-empty:

```text
DAHL_API_KEY
DAHL_BASE_URL
DAHL_MODEL
```

Example:

```bash
export DAHL_API_KEY="your-key"
export DAHL_MODEL="MiniMaxAI/MiniMax-M2.7"
cargo run --release
```

Invalid JSON or empty `model` / `base_url` values produce an error instead of
falling back silently.

The default base URL does not need to be present in the config file. Set
`DAHL_BASE_URL` (or `base_url`) only for another OpenAI-compatible gateway.
The endpoint must support `POST /chat/completions` with `stream: true`.

Confirm available model IDs with the provider's `GET /v1/models` endpoint.

## Usage

Start typing in the composer and press Enter to send. Responses stream into
the transcript as tokens arrive. A long generation is not killed by a short
total timeout; the client times out only on connect failure or a stretch of
inactivity.

### Global keys

| Key | Action |
| --- | --- |
| `Enter` | Send the prompt, or run a slash command |
| `Shift+Enter` or `Alt+Enter` | Insert a newline |
| `Esc` | Close overlay, interrupt a stream, or clear the composer |
| `Ctrl+T` | Conversation history |
| `Ctrl+G` | Browse and copy code blocks |
| `PageUp` / `PageDown` | Scroll the transcript |
| `?` (empty composer) | Keyboard shortcuts |
| `Ctrl+C` twice | Quit |

### Composer

| Key | Action |
| --- | --- |
| Left / Right | Move the cursor |
| `Ctrl+Left` / `Ctrl+Right` | Jump by word |
| `Alt+B` / `Alt+F` | Jump by word |
| Home / End | Start / end of the prompt |
| Backspace / Delete | Delete backward / forward |
| `Ctrl+U` | Clear the composer |
| Up / Down | Prompt history (or slash-command list) |
| Tab | Accept the highlighted slash command |

### History overlay (`Ctrl+T` or `/history`)

| Key | Action |
| --- | --- |
| Up / Down | Select a conversation |
| PageUp / PageDown | Move by a page |
| Enter | Open the selected conversation |
| `d` | Delete the selected conversation (the last one cannot be deleted) |
| Esc | Close |

### Code overlay (`Ctrl+G` or `/code`)

| Key | Action |
| --- | --- |
| Up / Down | Select a fenced code block |
| Enter | Copy it to the clipboard |
| Esc | Close |

Clipboard copy prefers a system utility (`pbcopy`, `wl-copy`, `xclip`, `xsel`,
or `clip`) and falls back to OSC 52.

### Slash commands

| Command | Action |
| --- | --- |
| `/help` | Show keyboard shortcuts |
| `/new` | Start a new conversation |
| `/history` | Open the history overlay |
| `/code` | Open the code-block overlay |
| `/model <model-id>` | Switch the model for later requests |
| `/quit` | Exit |

## Sessions

Conversations are stored as JSON in the platform data directory for `chat-tui`:

```text
Linux:   ~/.local/share/chat-tui/sessions.json
macOS:   ~/Library/Application Support/chat-tui/sessions.json
Windows: %APPDATA%\chatTUI\chat-tui\sessions.json
```

If `XDG_DATA_HOME` is set on Linux, that location is used instead. A write
failure is shown in the transcript; the in-memory conversation is kept. If the
session file is missing, a new empty conversation is created. If it is
corrupted, startup fails with the file path so the file can be moved or
deleted without being overwritten.

The footer token figure is an estimate (`tok est.`), not an exact tokenizer
count.

## Troubleshooting

**The app says the API key is missing**

Set `DAHL_API_KEY` or put `api_key` in `dahl.json`.

**The model is rejected**

Check the spelling and confirm that the model is available through
`GET /v1/models`. Switch with `/model <model-id>` or `DAHL_MODEL`.

**The request fails**

HTTP errors include the status and, when the API returns one, the server
message (400, 401, 403, 404, 429, 5xx). Check the key, model, base URL, and
quota. A proxy must support OpenAI-compatible streaming responses.

**A key was exposed**

Revoke it with the provider and create a replacement.

## License

See [LICENSE](LICENSE).
