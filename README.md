# chatTUI

`chatTUI` is a small, fast terminal chat client for OpenAI-compatible APIs. It is built
in Rust and designed for people who prefer a focused keyboard workflow over a
browser window.

The app uses `ratatui` for the interface, `crossterm` for terminal input,
`tokio` for asynchronous work, and `reqwest` for native OpenAI-compatible SSE streaming.

## What You Get

- Live responses as the selected model generates them
- An always-on composer with a `Working` / `Thinking` status row while streaming
- Scrollable conversation view
- Local conversation history saved as JSON
- History drawer for returning to previous chats
- Configurable Dahl model, temperature, and API endpoint
- `/model` picker that lists the models the API actually offers (`GET /models`)
  and marks the active one
- Automatic fallback: if a model rejects the request or is at high demand,
  chatTUI switches to another available model and announces the switch
- Markdown-friendly response output with shaded code boxes
- Copy any code block from a response to the clipboard (`ctrl+g` or `/code`)
- Animated reasoning view for thinking models such as `MiniMaxAI/MiniMax-M2.7`
  (see [Reasoning Models](#reasoning-models))
- A single native Rust binary with no Python or OpenAI SDK dependency

## Before You Start

You need:

- Rust and Cargo from [rustup.rs](https://rustup.rs/)
- An API key from [Dahl Inference](https://inference.dahl.global/) and/or [APInex](https://api.apinex.bond/v1)

Check your Rust installation:

```bash
rustc --version
cargo --version
```

## Install

Clone the repository and enter the project directory:

```bash
git clone https://github.com/AmirRanger149/chatTUI.git
cd chatTUI
```

Build the optimized release binary:

```bash
cargo build --release
```

Or run directly while developing:

```bash
cargo run
```

## Configuration (`config.json`)

`chatTUI` looks for `config.json` in the application directory or the current working directory.

### Full & Complete `config.json` Template

Create a `config.json` file in your root folder:

```json
{
  "dahl_api_key": "your-dahl-key-here",
  "apinex_api_key": "sk-apx-your-apinex-key-here",
  "temperature": 0.7
}
```

### Single Provider Examples

If you only use one provider, you can include just that key:

**For APInex:**
```json
{
  "apinex_api_key": "sk-apx-your-key-here"
}
```

**For Dahl:**
```json
{
  "dahl_api_key": "your-dahl-key-here"
}
```

> **Note:** If only one API key is present in `config.json`, chatTUI will automatically set that provider as the active default on startup. If both keys are present, Dahl is selected by default and you can switch between them anytime using the `/provider` command.

### Configuration Fields

| Field | Description | Default |
| --- | --- | --- |
| `dahl_api_key` | API key for Dahl Inference | `None` (or `DAHL_API_KEY` env) |
| `apinex_api_key` | API key for APInex (`sk-apx...`) | `None` (or `APINEX_API_KEY` env) |
| `temperature` | Sampling temperature for responses | `0.7` |
| `model` | Optional model name override | Provider default |

### Environment Variables (Alternative)

You can also export environment variables instead of creating a `config.json`:

```bash
# Dahl
export DAHL_API_KEY="your-dahl-key"

# APInex
export APINEX_API_KEY="sk-apx..."

cargo run --release
```


## Using chatTUI

The composer is always focused — just start typing and press `Enter` to send.

### Keyboard

| Key | Action |
| --- | --- |
| `Enter` | Send the message |
| `Shift+Enter` | Newline in the composer |
| `Esc` | Close a popup, then interrupt a running stream, then clear the composer |
| `Ctrl+T` | Conversation history |
| `Ctrl+G` | Browse and copy code blocks |
| `Ctrl+R` | Show / hide model reasoning |
| `PgUp` / `PgDn` | Scroll the transcript |
| `Up` / `Down` | Recall previous prompts |
| `←` / `→` | Move the cursor (`Ctrl` jumps whole words) |
| `Ctrl+U` | Clear the composer |
| `?` | Keyboard shortcuts |
| `Ctrl+C` ×2 | Quit |

### Slash commands

| Command | Action |
| --- | --- |
| `/help` | Show keyboard shortcuts |
| `/new` | Start a new conversation |
| `/history` | Browse saved conversations |
| `/code` | Browse and copy code blocks |
| `/model` | Pick a model from the API's live list (`/model <id>` sets one directly) |
| `/provider` | Select API provider (`/provider <name>` sets one directly) |
| `/quit` | Exit chatTUI |

Type `/` to open the command palette, then `Tab` to complete.

### Clipboard

`ctrl+g` / `/code` copies through your system clipboard when a clipboard tool
is available — `pbcopy` on macOS, `clip` on Windows, `wl-copy` / `xclip` /
`xsel` on Linux — so the copy is verified. Otherwise chatTUI falls back to the
terminal's OSC 52 sequence (with tmux/screen passthrough, and automatically
over SSH), which is best-effort: if pasting comes up empty, install `wl-copy`
(Wayland) or `xclip` (X11), or enable OSC 52 clipboard support in your
terminal.

## Reasoning Models

Some models expose their private chain of thought by wrapping it in
`<think> … </think>` before the actual answer. `MiniMaxAI/MiniMax-M2.7` — the
default model — is one of them. chatTUI understands that format and gives it
its own animated treatment instead of dumping raw tags into the transcript.

**While the model is thinking**, the status row above the composer turns into a
shimmering indicator with a live preview of the thought being written:

```text
✻ Thinking (7s • esc to interrupt)  comparing the two approaches
```

The transcript shows the reasoning as a dim, italic block behind a `┃` rule,
kept to the last few lines so it never pushes the answer off screen.

**Once the answer starts**, the block collapses into a single quiet summary line:

```text
✻ Thought for 84 words  ▸ ctrl+r
```

Press `Ctrl+R` at any time to expand or collapse completed reasoning blocks.

Notes:

- Reasoning is never replayed back to the API on later turns, so it does not
  consume context or confuse the model.
- Interrupting a stream mid-thought (`Esc`) still leaves a tidy, collapsible block.
- Models that do not emit `<think>` tags are unaffected — you get the usual
  `• Working` indicator and plain markdown output.

## Models And Endpoints

The default model is `MiniMaxAI/MiniMax-M2.7`, a reasoning model — see
[Reasoning Models](#reasoning-models). Change it with `DAHL_MODEL` or
the `model` value in your config file. The model name must be available through
Dahl; retrieve current model IDs from its `GET /v1/models` endpoint.

### Availability-Based Default (APInex)

APInex serves a rotating list of models — including free models published
under the `free/` namespace, e.g. `free/deepseek-v4-flash-0731` — so a
hardcoded default model can disappear or be replaced. When APInex is the
active provider (at startup, or after `/provider apinex`), chatTUI fetches
its live `GET /models` list in the background and sets the default model
from what is actually available:

1. the first free model (`free/…`), or
2. the built-in default if it is still offered, or
3. the first model in the live list.

The pick is announced in the transcript (e.g.
`APInex default set to available free model: free/deepseek-v4-flash-0731`).
An explicit choice always wins — a model set with `APINEX_MODEL`, the
`model` config field, or `/model` is never overridden — and if the fetch
fails the built-in default stays in effect, with the usual send-time
fallback covering a model that turns out to be unavailable. The fetched
list doubles as the cached list shown by the `/model` picker.

### The `/model` Picker

Run `/model` with no argument and chatTUI queries the endpoint's `GET /models`,
then shows what is actually available in a popup: your current model is marked
`· active` and preselected, `↑↓` (or `PgUp` / `PgDn`) move through the list,
and `Enter` switches to the highlighted model. Press `r` to refetch the list
from the API and `esc` to close. The list is cached for five minutes so
reopening the picker is instant.

`/model <id>` still sets a model directly. When a fetched list is cached, the
id is resolved against it — exact match (case-insensitive), then a unique
prefix, then a unique suffix — so both `/model minimaxai/minimax-m2.7` and the
shorthand `/model minimax-m2.7` find `MiniMaxAI/MiniMax-M2.7`. Anything
ambiguous or unknown is set exactly as typed.

### Automatic Model Fallback

When a send fails because of the model — a bad request against it, a
model that no longer exists, throttling, or the classic "currently
experiencing high demand" overload — chatTUI fetches the endpoint's model
list, picks another available model (preferring one from the same family,
e.g. another `MiniMaxAI/…`), and retries. Every switch is announced in the
transcript, so you always know which model answered:

```text
⚠ MiniMaxAI/MiniMax-M2.7 is unavailable — the model rejected the request (HTTP 429: rate limit exceeded)
  switching to MiniMaxAI/MiniMax-M1
```

Up to three fallbacks are tried per message, and the request is only retried
before any output has been written — a stream that breaks mid-answer is
reported as-is instead of being spliced onto a second model. Authentication
failures are reported immediately, since a different model cannot fix those.

The client uses this endpoint internally by default, so it does not need to be
present in `config.json`:

```text
https://inference.dahl.global/v1
```

You may set `DAHL_BASE_URL` for another OpenAI-compatible gateway or proxy. The
endpoint must support:

```text
POST /chat/completions
```

## Saved Data

Conversation history is stored in the platform data directory, normally:

```text
~/.local/share/chatTUI/chat-tui/sessions.json
```

The history file contains your saved messages. Back it up if you need to keep
your conversations, and protect it if they contain private information.

## Troubleshooting

**The app says the API key is missing**

Set `DAHL_API_KEY` or create the JSON configuration file in the location
above.

**The model is rejected**

Type `/model` to pick from the list the API actually offers. If a request is
rejected by a model that is overloaded or no longer available, chatTUI
automatically retries with another available model and tells you in the
transcript.

**The request fails or times out**

Check your network connection, API quota, endpoint URL, and API key. A proxy
must support OpenAI-compatible streaming responses.

**A key was exposed**

Revoke it in Dahl and create a replacement.

## License

See [LICENSE](LICENSE).
