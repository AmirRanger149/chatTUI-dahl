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
- Markdown-friendly response output with shaded code boxes
- Copy any code block from a response to the clipboard (`ctrl+g` or `/code`)
- Animated reasoning view for thinking models such as `MiniMaxAI/MiniMax-M2.7`
  (see [Reasoning Models](#reasoning-models))
- A single native Rust binary with no Python or OpenAI SDK dependency

## Before You Start

You need:

- Rust and Cargo from [rustup.rs](https://rustup.rs/)
- A Dahl API key from [Dahl Inference](https://inference.dahl.global/)

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

## Configure Your Dahl Key

### Environment variable

This is the quickest option:

```bash
export DAHL_API_KEY="your-key"
cargo run --release
```

You can put the export in your shell profile if you use the app regularly.

### `dahl.json`

The app looks for `dahl.json` beside the compiled application first. When
running with `cargo run`, it also checks the current working directory. Its
contents can look like this:

```json
{
  "api_key": "your-key",
  "model": "MiniMaxAI/MiniMax-M2.7",
  "temperature": 0.7
}
```

The API key is read from this file so you do not need to enter it each time.
Keep this file private and never commit it.

Environment variables take priority for the key-related values:

```text
DAHL_API_KEY
DAHL_BASE_URL
DAHL_MODEL
```

### Example file

```json
{
  "api_key": "your-key",
  "model": "MiniMaxAI/MiniMax-M2.7"
}
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
| `/model <id>` | Switch model |
| `/quit` | Exit chatTUI |

Type `/` to open the command palette, then `Tab` to complete.

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

Check the spelling and confirm that the model is available through `GET /v1/models`.

**The request fails or times out**

Check your network connection, API quota, endpoint URL, and API key. A proxy
must support OpenAI-compatible streaming responses.

**A key was exposed**

Revoke it in Dahl and create a replacement.

## License

See [LICENSE](LICENSE).
