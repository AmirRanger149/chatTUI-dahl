# chatTUI

`chatTUI` is a small, fast terminal chat client for OpenAI-compatible APIs. It is built
in Rust and designed for people who prefer a focused keyboard workflow over a
browser window.

The app uses `ratatui` for the interface, `crossterm` for terminal input,
`tokio` for asynchronous work, and `reqwest` for native OpenAI-compatible SSE streaming.

## What You Get

- Live responses as the selected model generates them
- Vim-style `NORMAL` and `INSERT` modes
- Scrollable conversation view
- Local conversation history saved as JSON
- History drawer for returning to previous chats
- Configurable Dahl model, temperature, and API endpoint
- Markdown-friendly response output with shaded code boxes
- Copy any code block from a response to the clipboard (`ctrl+g` or `/code`)
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

The app starts in `NORMAL` mode. Press `i` to begin writing a prompt.

### Normal mode

| Key | Action |
| --- | --- |
| `i` | Enter Insert mode |
| `j` or `Down` | Scroll down |
| `k` or `Up` | Scroll up |
| `h` | Toggle the history drawer |
| `Shift+H` | Switch to the next saved conversation |
| `n` | Start a new conversation |
| `?` | Show a help hint |
| `q` | Quit |

### Insert mode

| Key | Action |
| --- | --- |
| Any character | Add it to the prompt |
| `Enter` | Submit the prompt |
| `Backspace` | Delete the previous character |
| `Esc` | Return to Normal mode |

## Models And Endpoints

The default model is `MiniMaxAI/MiniMax-M2.7`. Change it with `DAHL_MODEL` or
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
