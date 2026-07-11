# LocalDrop File Transfer Protocol (LDFTP) — Specification v1.0

> **Revision:** 1.0.0  
> **Date:** 2026-07-10  
> **Scope:** Local-area network (LAN/Wi-Fi) file transfer between a Windows client (Rust) and an Android client (Kotlin).  
> **Status:** Stable

---

## Table of Contents

1. [Overview](#1-overview)
2. [Terminology](#2-terminology)
3. [Architecture](#3-architecture)
4. [Phase 1 — Device Discovery (UDP Broadcast)](#4-phase-1--device-discovery-udp-broadcast)
5. [Phase 2 — TCP Connection & Handshake](#5-phase-2--tcp-connection--handshake)
6. [Phase 3 — File Metadata Exchange](#6-phase-3--file-metadata-exchange)
7. [Phase 4 — Chunked File Transfer](#7-phase-4--chunked-file-transfer)
8. [Phase 5 — Progress Reporting](#8-phase-5--progress-reporting)
9. [Phase 6 — Transfer Completion & Teardown](#9-phase-6--transfer-completion--teardown)
10. [Error Handling](#10-error-handling)
11. [Security Considerations](#11-security-considerations)
12. [Implementation Notes — Rust (Windows)](#12-implementation-notes--rust-windows)
13. [Implementation Notes — Kotlin (Android)](#13-implementation-notes--kotlin-android)
14. [Full Message Reference](#14-full-message-reference)
15. [Example Session Transcript](#15-example-session-transcript)

---

## 1. Overview

**LocalDrop File Transfer Protocol (LDFTP)** is a simple, stateless-friendly application-layer protocol for transferring files between devices on the same local network. It does **not** require a central server.

The protocol operates in five sequential phases:

```
[Sender]                                [Receiver]
   |                                        |
   |----  UDP Broadcast (Discovery)  -----> |   Phase 1
   |<---- UDP Broadcast (Discovery)  -----  |
   |                                        |
   |====  TCP Connect (port from disc.) ==> |   Phase 2
   |----> HELLO handshake message    -----> |
   |<---- HELLO_ACK response         -----  |
   |                                        |
   |----> FILE_OFFER (metadata)      -----> |   Phase 3
   |<---- FILE_ACCEPT / FILE_REJECT  -----  |
   |                                        |
   |----> CHUNK (data)               -----> |   Phase 4
   |<---- CHUNK_ACK                  -----  |   (repeated)
   |                                        |
   |----> PROGRESS (optional)        -----> |   Phase 5
   |                                        |
   |----> TRANSFER_DONE              -----> |   Phase 6
   |<---- TRANSFER_COMPLETE          -----  |
   |                                        |
   |====  TCP Close                  ====   |
```

All control messages are **newline-delimited JSON** (one JSON object per line, terminated with `\n`).  
All binary payload data (file chunks) is sent as **raw bytes** immediately following the CHUNK control message line.

---

## 2. Terminology

| Term | Definition |
|------|-----------|
| **Sender** | The device initiating the file transfer. |
| **Receiver** | The device accepting and saving the file. |
| **Session** | One complete file transfer from HELLO to TRANSFER_COMPLETE. |
| **Chunk** | A 1 MB (1,048,576 byte) slice of the file being transferred. |
| **Transfer ID** | A UUID-v4 string uniquely identifying a transfer session. |
| **Checksum** | A SHA-256 hex-encoded digest of the complete file. |
| **Control Message** | A UTF-8 JSON line ending in `\n`. |
| **Payload** | Raw binary data sent immediately after a CHUNK control message. |

---

## 3. Architecture

### 3.1 Transport Layer Summary

| Protocol | Port | Purpose |
|----------|------|---------|
| UDP | `42000` | Device discovery broadcasts |
| TCP | `42001` | All control messages and file data |

> **Ports are fixed defaults.** Both sides MUST use these defaults unless a `port` field in the discovery message advertises a different TCP port.

### 3.2 Encoding Rules

- All control messages are **UTF-8 encoded JSON objects**, each terminated by a single newline byte (`0x0A`).
- JSON keys are `snake_case` strings.
- Integers are represented as JSON numbers (no quotes).
- Strings containing file paths use forward slashes or the platform's native separator — receivers MUST strip any directory component and use only the base filename when saving.
- Binary chunk data is **raw bytes**, NOT base64 encoded.

### 3.3 Role Determination

Either device can be a Sender or a Receiver in any given session. Roles are determined by who initiates the TCP connection:

- The device that opens the TCP connection is the **Sender**.
- The device that accepts the TCP connection is the **Receiver**.

---

## 4. Phase 1 — Device Discovery (UDP Broadcast)

### 4.1 Purpose

Devices announce their presence and TCP port on the local network via periodic UDP broadcast messages. This allows peers to discover each other without manual IP entry.

### 4.2 UDP Discovery Packet Format

Both devices broadcast a JSON object on UDP port `42000` to the subnet broadcast address (e.g., `192.168.1.255` or `255.255.255.255`).

**Schema:**

```json
{
  "protocol":    "LDFTP",
  "version":     "1.0",
  "type":        "DISCOVERY",
  "device_name": "<string>",
  "device_id":   "<uuid-v4>",
  "ip":          "<ipv4-address>",
  "port":        42001,
  "platform":    "<string>",
  "timestamp":   "<iso8601-utc>"
}
```

**Field Definitions:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `protocol` | string | YES | Always `"LDFTP"`. Used to filter non-LDFTP broadcasts. |
| `version` | string | YES | Protocol version. Currently `"1.0"`. |
| `type` | string | YES | Always `"DISCOVERY"` for discovery packets. |
| `device_name` | string | YES | Human-readable device name (e.g., `"Alice's Laptop"`, `"Galaxy S24"`). Max 64 chars. |
| `device_id` | string | YES | UUID-v4 that uniquely identifies this device installation. Persisted across restarts. |
| `ip` | string | YES | IPv4 address of the broadcasting interface. Must match the interface used for TCP. |
| `port` | integer | YES | TCP port where this device accepts inbound connections. |
| `platform` | string | YES | `"windows"`, `"android"`, `"linux"`, or `"macos"`. |
| `timestamp` | string | YES | ISO 8601 UTC timestamp of the broadcast (e.g., `"2026-07-10T05:17:00Z"`). |

**Example broadcast packet:**

```json
{
  "protocol":    "LDFTP",
  "version":     "1.0",
  "type":        "DISCOVERY",
  "device_name": "Alice's Laptop",
  "device_id":   "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
  "ip":          "192.168.1.42",
  "port":        42001,
  "platform":    "windows",
  "timestamp":   "2026-07-10T05:17:00Z"
}
```

### 4.3 Discovery Behavior Rules

1. Devices MUST broadcast every **5 seconds** while the application is running and discoverable.
2. Devices MUST listen on UDP port `42000` and parse incoming packets.
3. Packets with `protocol != "LDFTP"` or `version != "1.0"` MUST be silently ignored.
4. A device MUST ignore its own broadcasts (match by `device_id`).
5. Devices SHOULD maintain an in-memory peer list. Peers not heard from within **15 seconds** are considered offline and removed.
6. The UDP socket MUST be bound with `SO_REUSEADDR` and `SO_BROADCAST` enabled.
7. UDP payload size MUST NOT exceed **1024 bytes**. Truncate `device_name` if necessary.

### 4.4 Network Interface Selection

- On **Windows (Rust)**: Enumerate network interfaces and select the first non-loopback, non-virtual IPv4 interface. Derive broadcast address from IP + subnet mask.
- On **Android (Kotlin)**: Use `WifiManager.getDhcpInfo()` to get the broadcast address, or fall back to `255.255.255.255`.

---

## 5. Phase 2 — TCP Connection & Handshake

### 5.1 Opening the Connection

Once the Sender has discovered the Receiver via UDP, the Sender opens a TCP connection to the Receiver's `ip:port` from the discovery packet.

- Connection timeout: **10 seconds**.
- If the connection is refused or times out, the Sender SHOULD display an error and not retry automatically.

### 5.2 HELLO Message (Sender -> Receiver)

Immediately after the TCP connection is established, the Sender sends a HELLO message.

**Schema:**

```json
{
  "type":        "HELLO",
  "transfer_id": "<uuid-v4>",
  "sender_name": "<string>",
  "sender_id":   "<uuid-v4>",
  "version":     "1.0"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | YES | Always `"HELLO"`. |
| `transfer_id` | string | YES | UUID-v4 generated fresh for this transfer session. |
| `sender_name` | string | YES | Human-readable name of the sending device. |
| `sender_id` | string | YES | Persistent device UUID of the sender. |
| `version` | string | YES | Protocol version. Must be `"1.0"`. |

**Example:**

```json
{"type":"HELLO","transfer_id":"f47ac10b-58cc-4372-a567-0e02b2c3d479","sender_name":"Alice's Laptop","sender_id":"a1b2c3d4-e5f6-7890-abcd-ef1234567890","version":"1.0"}
```

### 5.3 HELLO_ACK Message (Receiver -> Sender)

The Receiver responds with a HELLO_ACK to confirm it is ready.

**Schema:**

```json
{
  "type":          "HELLO_ACK",
  "transfer_id":   "<uuid-v4>",
  "receiver_name": "<string>",
  "receiver_id":   "<uuid-v4>",
  "status":        "ready"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | YES | Always `"HELLO_ACK"`. |
| `transfer_id` | string | YES | Must echo the `transfer_id` from the HELLO message. |
| `receiver_name` | string | YES | Human-readable name of the receiving device. |
| `receiver_id` | string | YES | Persistent device UUID of the receiver. |
| `status` | string | YES | `"ready"` indicates the receiver is prepared. Any other value is an error. |

**Example:**

```json
{"type":"HELLO_ACK","transfer_id":"f47ac10b-58cc-4372-a567-0e02b2c3d479","receiver_name":"Bob's Phone","receiver_id":"b2c3d4e5-f6a7-8901-bcde-f12345678901","status":"ready"}
```

### 5.4 Handshake Error Cases

| Condition | Action |
|-----------|--------|
| `version` mismatch | Receiver sends `HELLO_ACK` with `"status": "version_mismatch"` and closes the connection. |
| `transfer_id` echo mismatch | Sender closes connection immediately. |
| No response within **10 seconds** | Sender closes connection and reports timeout. |

---

## 6. Phase 3 — File Metadata Exchange

### 6.1 FILE_OFFER Message (Sender -> Receiver)

After a successful handshake, the Sender describes the file it wants to send.

**Schema:**

```json
{
  "type":         "FILE_OFFER",
  "transfer_id":  "<uuid-v4>",
  "file_name":    "<string>",
  "file_size":    "<integer>",
  "checksum":     "<sha256-hex>",
  "mime_type":    "<string>",
  "chunk_size":   1048576,
  "total_chunks": "<integer>"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | YES | Always `"FILE_OFFER"`. |
| `transfer_id` | string | YES | Must match the session `transfer_id`. |
| `file_name` | string | YES | Base filename only (e.g., `"photo.jpg"`). No path components. Max 255 chars. |
| `file_size` | integer | YES | Total file size in bytes. |
| `checksum` | string | YES | SHA-256 hex digest of the complete, unmodified file. Lowercase hex, 64 chars. |
| `mime_type` | string | NO | MIME type string (e.g., `"image/jpeg"`). Optional; use `"application/octet-stream"` if unknown. |
| `chunk_size` | integer | YES | Size of each chunk in bytes. MUST be `1048576` (1 MiB) for this version. |
| `total_chunks` | integer | YES | `ceil(file_size / chunk_size)`. Last chunk may be smaller. |

**Checksum Calculation:**

```
SHA-256( entire_file_bytes ) -> hex string (lowercase)
```

**total_chunks Calculation:**

```
total_chunks = ceil(file_size / 1048576)
             = (file_size + 1048575) / 1048576   # integer math
```

**Example:**

```json
{"type":"FILE_OFFER","transfer_id":"f47ac10b-58cc-4372-a567-0e02b2c3d479","file_name":"vacation.mp4","file_size":52428800,"checksum":"e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855","mime_type":"video/mp4","chunk_size":1048576,"total_chunks":50}
```

### 6.2 FILE_ACCEPT Message (Receiver -> Sender)

The Receiver displays a prompt to the user. If the user accepts:

**Schema:**

```json
{
  "type":        "FILE_ACCEPT",
  "transfer_id": "<uuid-v4>",
  "resume_from": "<integer>"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | YES | Always `"FILE_ACCEPT"`. |
| `transfer_id` | string | YES | Must match the session `transfer_id`. |
| `resume_from` | integer | YES | Chunk index (0-based) to start from. `0` for a fresh transfer. Non-zero enables resumption. |

**Example (fresh transfer):**

```json
{"type":"FILE_ACCEPT","transfer_id":"f47ac10b-58cc-4372-a567-0e02b2c3d479","resume_from":0}
```

### 6.3 FILE_REJECT Message (Receiver -> Sender)

If the user declines, or if there is insufficient storage:

**Schema:**

```json
{
  "type":        "FILE_REJECT",
  "transfer_id": "<uuid-v4>",
  "reason":      "<string>"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | YES | Always `"FILE_REJECT"`. |
| `transfer_id` | string | YES | Must match the session `transfer_id`. |
| `reason` | string | YES | Human-readable or machine-readable reason. One of: `"user_declined"`, `"insufficient_storage"`, `"file_exists"`, `"error"`. |

**Example:**

```json
{"type":"FILE_REJECT","transfer_id":"f47ac10b-58cc-4372-a567-0e02b2c3d479","reason":"user_declined"}
```

> Upon receiving `FILE_REJECT`, the Sender MUST close the TCP connection immediately.

### 6.4 Offer Timeout

If the Receiver does not respond to FILE_OFFER within **60 seconds**, the Sender SHOULD close the connection (user may have dismissed the dialog).

---

## 7. Phase 4 — Chunked File Transfer

### 7.1 Chunk Transmission Flow

After receiving `FILE_ACCEPT`, the Sender transmits chunks in order. Each chunk is sent as:

1. A **CHUNK control message** (JSON line terminated by `\n`)
2. Immediately followed by the **raw binary payload** of exactly `chunk_size` bytes (or fewer for the last chunk)

The Receiver MUST send a `CHUNK_ACK` after successfully writing each chunk to disk before the Sender transmits the next chunk. This provides **stop-and-wait** flow control that prevents buffer overflow on constrained devices.

```
Sender                              Receiver
  |                                    |
  |---[CHUNK JSON]\n[binary data]----> |  chunk_index=0
  |<---[CHUNK_ACK JSON]\n-----------  |
  |                                    |
  |---[CHUNK JSON]\n[binary data]----> |  chunk_index=1
  |<---[CHUNK_ACK JSON]\n-----------  |
  |          ... (repeat) ...          |
  |---[CHUNK JSON]\n[binary data]----> |  chunk_index=N-1 (last)
  |<---[CHUNK_ACK JSON]\n-----------  |
```

### 7.2 CHUNK Control Message (Sender -> Receiver)

**Schema:**

```json
{
  "type":           "CHUNK",
  "transfer_id":    "<uuid-v4>",
  "chunk_index":    "<integer>",
  "chunk_size":     "<integer>",
  "chunk_checksum": "<sha256-hex>"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | YES | Always `"CHUNK"`. |
| `transfer_id` | string | YES | Must match the session `transfer_id`. |
| `chunk_index` | integer | YES | 0-based index of this chunk. Must be sequential. |
| `chunk_size` | integer | YES | Exact byte count of the payload that immediately follows this JSON line. |
| `chunk_checksum` | string | YES | SHA-256 hex digest of this chunk's raw bytes only. |

**Example:**

```json
{"type":"CHUNK","transfer_id":"f47ac10b-58cc-4372-a567-0e02b2c3d479","chunk_index":0,"chunk_size":1048576,"chunk_checksum":"abc123..."}
```

### 7.3 Reading the Binary Payload

After parsing the CHUNK JSON line (reading up to and including `\n`), the receiver MUST:

1. Read exactly `chunk_size` bytes from the TCP stream (blocking read).
2. Compute SHA-256 of the received bytes.
3. Compare with `chunk_checksum`. If mismatch, send `CHUNK_NAK`.
4. Write bytes to the output file at offset `chunk_index * 1048576`.
5. Send `CHUNK_ACK`.

> **Important:** Do NOT attempt to read more bytes than `chunk_size`. The next control message begins immediately after the last byte of the payload.

### 7.4 CHUNK_ACK Message (Receiver -> Sender)

**Schema:**

```json
{
  "type":        "CHUNK_ACK",
  "transfer_id": "<uuid-v4>",
  "chunk_index": "<integer>"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | YES | Always `"CHUNK_ACK"`. |
| `transfer_id` | string | YES | Must match the session `transfer_id`. |
| `chunk_index` | integer | YES | Must echo the `chunk_index` from the corresponding CHUNK message. |

**Example:**

```json
{"type":"CHUNK_ACK","transfer_id":"f47ac10b-58cc-4372-a567-0e02b2c3d479","chunk_index":0}
```

### 7.5 CHUNK_NAK Message (Receiver -> Sender)

If the chunk checksum fails, the Receiver sends a NAK. The Sender MUST retransmit the same chunk.

**Schema:**

```json
{
  "type":        "CHUNK_NAK",
  "transfer_id": "<uuid-v4>",
  "chunk_index": "<integer>",
  "reason":      "<string>"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | YES | Always `"CHUNK_NAK"`. |
| `transfer_id` | string | YES | Must match the session `transfer_id`. |
| `chunk_index` | integer | YES | The failing chunk index. |
| `reason` | string | YES | `"checksum_mismatch"` or `"write_error"`. |

**Retransmission Rules:**
- Sender MUST retransmit the same chunk up to **3 times**.
- If 3 consecutive NAKs are received for the same chunk, the Sender MUST send `TRANSFER_ERROR` and close the connection.

---

## 8. Phase 5 — Progress Reporting

### 8.1 PROGRESS Message (Sender -> Receiver)

The Sender SHOULD send a PROGRESS message after every chunk ACK. This is optional but RECOMMENDED for UI updates.

**Schema:**

```json
{
  "type":         "PROGRESS",
  "transfer_id":  "<uuid-v4>",
  "chunks_sent":  "<integer>",
  "total_chunks": "<integer>",
  "bytes_sent":   "<integer>",
  "total_bytes":  "<integer>",
  "elapsed_ms":   "<integer>",
  "speed_bps":    "<integer>"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `type` | string | YES | Always `"PROGRESS"`. |
| `transfer_id` | string | YES | Session ID. |
| `chunks_sent` | integer | YES | Number of chunks successfully ACK'd so far. |
| `total_chunks` | integer | YES | Total chunks in the transfer. |
| `bytes_sent` | integer | YES | Total bytes transferred so far (sum of ACK'd chunk sizes). |
| `total_bytes` | integer | YES | Total file size in bytes. |
| `elapsed_ms` | integer | YES | Milliseconds elapsed since CHUNK index 0 was sent. |
| `speed_bps` | integer | YES | Current transfer speed in bytes per second. Calculated as `bytes_sent * 1000 / elapsed_ms`. |

> The Receiver does NOT reply to PROGRESS messages. It may use them to update a local progress UI.

**Example:**

```json
{"type":"PROGRESS","transfer_id":"f47ac10b-58cc-4372-a567-0e02b2c3d479","chunks_sent":10,"total_chunks":50,"bytes_sent":10485760,"total_bytes":52428800,"elapsed_ms":3200,"speed_bps":3276800}
```

---

## 9. Phase 6 — Transfer Completion & Teardown

### 9.1 TRANSFER_DONE Message (Sender -> Receiver)

After all chunks have been ACK'd, the Sender sends TRANSFER_DONE.

**Schema:**

```json
{
  "type":        "TRANSFER_DONE",
  "transfer_id": "<uuid-v4>",
  "file_name":   "<string>",
  "file_size":   "<integer>",
  "checksum":    "<sha256-hex>"
}
```

The `checksum` here is the same whole-file SHA-256 sent in `FILE_OFFER`. The Receiver MUST recompute the checksum of the received file and compare.

### 9.2 TRANSFER_COMPLETE Message (Receiver -> Sender)

If the file checksum matches:

**Schema:**

```json
{
  "type":        "TRANSFER_COMPLETE",
  "transfer_id": "<uuid-v4>",
  "status":      "success"
}
```

### 9.3 TRANSFER_ERROR Message (Either Direction)

If an unrecoverable error occurs at any point:

**Schema:**

```json
{
  "type":        "TRANSFER_ERROR",
  "transfer_id": "<uuid-v4>",
  "error_code":  "<string>",
  "message":     "<string>"
}
```

| `error_code` | Meaning |
|--------------|---------|
| `"checksum_mismatch"` | Final file checksum does not match expected. |
| `"chunk_retries_exceeded"` | 3 consecutive NAKs on a single chunk. |
| `"connection_lost"` | TCP connection dropped unexpectedly. |
| `"disk_full"` | No space left on receiver's device. |
| `"timeout"` | An operation exceeded its time limit. |
| `"protocol_error"` | Unexpected message type or malformed JSON. |
| `"user_cancelled"` | The local user cancelled the transfer mid-flight. |

Upon sending or receiving `TRANSFER_ERROR`, both sides MUST close the TCP connection within **2 seconds**.

### 9.4 TCP Teardown

1. After `TRANSFER_COMPLETE` or `TRANSFER_ERROR` is exchanged, the **Sender closes the write side** (TCP FIN).
2. The Receiver drains any remaining data, then closes.
3. Both sides free all session resources.

---

## 10. Error Handling

### 10.1 Malformed JSON

If a received line cannot be parsed as valid JSON, or if a required field is missing:

- The receiving side MUST send `TRANSFER_ERROR` with `error_code: "protocol_error"`.
- The connection MUST be closed immediately after.

### 10.2 Unexpected Message Type

If a message of an unexpected `type` is received for the current protocol state (e.g., receiving a `CHUNK` before `FILE_ACCEPT`), treat it as a protocol error.

### 10.3 TCP Read Timeouts

| Phase | Timeout | Action on Expiry |
|-------|---------|-----------------|
| HELLO_ACK | 10 s | Close connection |
| FILE_ACCEPT / FILE_REJECT | 60 s | Close connection |
| CHUNK_ACK / CHUNK_NAK | 30 s | Retry or error |
| TRANSFER_COMPLETE | 15 s | Close connection |

### 10.4 Connection Drop Mid-Transfer

If the TCP connection is dropped unexpectedly:
- The Receiver SHOULD keep its partial file on disk.
- On reconnection, the Receiver MAY use `resume_from` in `FILE_ACCEPT` to resume.
- The Sender SHOULD detect the drop within **5 seconds** via TCP keep-alive or write failure.

---

## 11. Security Considerations

> **Warning:** This protocol is designed for **trusted local networks only**. It provides NO encryption, NO authentication, and NO authorization beyond user acceptance of a file offer.

| Risk | Mitigation |
|------|-----------|
| Rogue device spoofing a discovery packet | User is shown `device_name` and must manually accept each file offer. |
| Man-in-the-middle on LAN | Out of scope for v1.0. Future versions may add TLS. |
| Malicious filenames (path traversal) | Receiver MUST strip all directory components from `file_name` and save only in the designated downloads directory. |
| Large file DoS | Receiver SHOULD enforce a configurable max file size (e.g., 10 GB). |
| Port scanning | Ports 42000/42001 are non-standard; the risk is the same as any LAN service. |

---

## 12. Implementation Notes — Rust (Windows)

### 12.1 Recommended Crates

| Crate | Purpose |
|-------|---------|
| `tokio` | Async runtime for TCP/UDP |
| `tokio::net::UdpSocket` | UDP discovery |
| `tokio::net::TcpListener` / `TcpStream` | TCP server & client |
| `serde` + `serde_json` | JSON serialization/deserialization |
| `sha2` | SHA-256 checksum |
| `uuid` | UUID-v4 generation |
| `tokio::io::BufReader` | Line-by-line JSON reading |

### 12.2 Reading Control Messages in Rust

```rust
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::TcpStream;

let stream = TcpStream::connect("192.168.1.100:42001").await?;
let (reader, writer) = stream.into_split();
let mut buf_reader = BufReader::new(reader);

let mut line = String::new();
buf_reader.read_line(&mut line).await?;
let msg: serde_json::Value = serde_json::from_str(&line)?;
```

### 12.3 Sending Binary Payload in Rust

```rust
use tokio::io::AsyncWriteExt;

// Send CHUNK control message
let chunk_msg = serde_json::to_string(&chunk_json)? + "\n";
writer.write_all(chunk_msg.as_bytes()).await?;

// Immediately send raw bytes
writer.write_all(&chunk_bytes).await?;
writer.flush().await?;
```

### 12.4 Reading Binary Payload in Rust

```rust
use tokio::io::AsyncReadExt;

// After reading the CHUNK JSON line:
let mut payload = vec![0u8; chunk_size as usize];
buf_reader.read_exact(&mut payload).await?;
```

### 12.5 UDP Broadcast in Rust

```rust
use tokio::net::UdpSocket;

let sock = UdpSocket::bind("0.0.0.0:42000").await?;
sock.set_broadcast(true)?;
sock.send_to(packet.as_bytes(), "255.255.255.255:42000").await?;
```

### 12.6 SHA-256 Checksum in Rust

```rust
use sha2::{Sha256, Digest};

let mut hasher = Sha256::new();
hasher.update(&file_bytes);
let result = hasher.finalize();
let hex = format!("{:x}", result);
```

---

## 13. Implementation Notes — Kotlin (Android)

### 13.1 Recommended Libraries

| Library | Purpose |
|---------|---------|
| `kotlinx.coroutines` | Async/coroutine support |
| `java.net.DatagramSocket` | UDP broadcast |
| `java.net.ServerSocket` / `Socket` | TCP server & client |
| `org.json.JSONObject` | JSON parsing (built-in Android) |
| `java.security.MessageDigest` | SHA-256 |
| `java.util.UUID` | UUID generation |
| `java.io.BufferedReader` | Line-by-line JSON reading |

### 13.2 Reading Control Messages in Kotlin

```kotlin
import java.net.Socket
import java.io.BufferedReader
import java.io.InputStreamReader

val socket = Socket("192.168.1.42", 42001)
val reader = BufferedReader(InputStreamReader(socket.getInputStream(), Charsets.UTF_8))
val writer = socket.getOutputStream()

// Read one JSON line
val line = reader.readLine() ?: throw IOException("Connection closed")
val msg = org.json.JSONObject(line)
```

### 13.3 Sending Binary Payload in Kotlin

```kotlin
import java.io.OutputStreamWriter

val textWriter = OutputStreamWriter(socket.getOutputStream(), Charsets.UTF_8)
val binaryStream = socket.getOutputStream()

// Send CHUNK control message
textWriter.write(chunkJson.toString() + "\n")
textWriter.flush()

// Immediately send raw chunk bytes
binaryStream.write(chunkBytes)
binaryStream.flush()
```

### 13.4 Reading Binary Payload in Kotlin

```kotlin
val inputStream = socket.getInputStream()
val reader = BufferedReader(InputStreamReader(inputStream, Charsets.UTF_8))

// After reading JSON line with reader.readLine():
// Switch to raw byte reading via the underlying stream
val payload = ByteArray(chunkSize)
var bytesRead = 0
while (bytesRead < chunkSize) {
    val n = inputStream.read(payload, bytesRead, chunkSize - bytesRead)
    if (n == -1) throw IOException("Stream ended prematurely")
    bytesRead += n
}
```

> **Caution:** On Android, `BufferedReader` may buffer ahead and consume bytes that belong to the binary payload. Use a dedicated `DataInputStream` with `readFully()` or manage a single raw `InputStream` with manual JSON line parsing to avoid this buffer-mixing issue.

### 13.5 UDP Broadcast in Kotlin

```kotlin
import java.net.DatagramSocket
import java.net.DatagramPacket
import java.net.InetAddress

val socket = DatagramSocket()
socket.broadcast = true
val data = discoveryJson.toByteArray(Charsets.UTF_8)
val packet = DatagramPacket(
    data, data.size,
    InetAddress.getByName("255.255.255.255"), 42000
)
socket.send(packet)
```

### 13.6 SHA-256 Checksum in Kotlin

```kotlin
import java.security.MessageDigest

fun sha256(bytes: ByteArray): String {
    val digest = MessageDigest.getInstance("SHA-256")
    return digest.digest(bytes).joinToString("") { "%02x".format(it) }
}
```

### 13.7 Android Permissions Required

Add to `AndroidManifest.xml`:

```xml
<uses-permission android:name="android.permission.INTERNET" />
<uses-permission android:name="android.permission.ACCESS_WIFI_STATE" />
<uses-permission android:name="android.permission.CHANGE_WIFI_MULTICAST_STATE" />
<uses-permission android:name="android.permission.READ_EXTERNAL_STORAGE" />
<uses-permission android:name="android.permission.WRITE_EXTERNAL_STORAGE" />
```

> On Android 10+ (API 29+), use `MediaStore` API for writing to shared storage instead of direct file paths.

---

## 14. Full Message Reference

### Quick Reference Table

| Message | Direction | Phase | Description |
|---------|-----------|-------|-------------|
| `DISCOVERY` | Broadcast | 1 | UDP peer announcement |
| `HELLO` | Sender -> Receiver | 2 | Open session handshake |
| `HELLO_ACK` | Receiver -> Sender | 2 | Confirm readiness |
| `FILE_OFFER` | Sender -> Receiver | 3 | File metadata proposal |
| `FILE_ACCEPT` | Receiver -> Sender | 3 | User accepts file |
| `FILE_REJECT` | Receiver -> Sender | 3 | User rejects file |
| `CHUNK` | Sender -> Receiver | 4 | Chunk header + binary payload |
| `CHUNK_ACK` | Receiver -> Sender | 4 | Chunk successfully received |
| `CHUNK_NAK` | Receiver -> Sender | 4 | Chunk failed, request retransmit |
| `PROGRESS` | Sender -> Receiver | 5 | Transfer progress update |
| `TRANSFER_DONE` | Sender -> Receiver | 6 | All chunks sent |
| `TRANSFER_COMPLETE` | Receiver -> Sender | 6 | File verified and saved |
| `TRANSFER_ERROR` | Either | Any | Unrecoverable error |

### State Machine

```
[Sender States]                     [Receiver States]

IDLE                                IDLE
  | (user picks file + peer)          | (listening on TCP port)
  v                                   v
CONNECTING                          ACCEPTING
  | TCP connect success               | TCP connection received
  v                                   v
HELLO_SENT                          HELLO_RECEIVED
  | HELLO_ACK received                | HELLO sent
  v                                   v
OFFER_SENT                          OFFER_RECEIVED
  | FILE_ACCEPT received              | (await user input)
  v                                   v
TRANSFERRING                        RECEIVING
  | all CHUNK_ACKs received           | all chunks written
  v                                   v
DONE_SENT                           VERIFYING
  | TRANSFER_COMPLETE received        | checksum verified
  v                                   v
IDLE                                IDLE
```

---

## 15. Example Session Transcript

Below is a complete annotated example of a 2-chunk (2 MiB) file transfer.

```
=== Phase 1: Discovery ===
[UDP 192.168.1.42:42000 -> BROADCAST]
{"protocol":"LDFTP","version":"1.0","type":"DISCOVERY","device_name":"Alice's Laptop","device_id":"aaa...","ip":"192.168.1.42","port":42001,"platform":"windows","timestamp":"2026-07-10T05:17:00Z"}

[UDP 192.168.1.100:42000 -> BROADCAST]
{"protocol":"LDFTP","version":"1.0","type":"DISCOVERY","device_name":"Bob's Phone","device_id":"bbb...","ip":"192.168.1.100","port":42001,"platform":"android","timestamp":"2026-07-10T05:17:02Z"}

=== Phase 2: TCP Handshake (Alice -> Bob) ===
[TCP CONNECT 192.168.1.42 -> 192.168.1.100:42001]

[SENDER -> RECEIVER]
{"type":"HELLO","transfer_id":"tid-001","sender_name":"Alice's Laptop","sender_id":"aaa...","version":"1.0"}

[RECEIVER -> SENDER]
{"type":"HELLO_ACK","transfer_id":"tid-001","receiver_name":"Bob's Phone","receiver_id":"bbb...","status":"ready"}

=== Phase 3: File Offer ===
[SENDER -> RECEIVER]
{"type":"FILE_OFFER","transfer_id":"tid-001","file_name":"report.pdf","file_size":2097152,"checksum":"deadbeef...","mime_type":"application/pdf","chunk_size":1048576,"total_chunks":2}

  [Bob's UI shows: "Alice wants to send report.pdf (2.0 MB) -- Accept?"]

[RECEIVER -> SENDER]
{"type":"FILE_ACCEPT","transfer_id":"tid-001","resume_from":0}

=== Phase 4: Chunk 0 ===
[SENDER -> RECEIVER]
{"type":"CHUNK","transfer_id":"tid-001","chunk_index":0,"chunk_size":1048576,"chunk_checksum":"aabbcc..."}
<< 1,048,576 raw bytes >>

[RECEIVER -> SENDER]
{"type":"CHUNK_ACK","transfer_id":"tid-001","chunk_index":0}

=== Phase 5: Progress ===
[SENDER -> RECEIVER]
{"type":"PROGRESS","transfer_id":"tid-001","chunks_sent":1,"total_chunks":2,"bytes_sent":1048576,"total_bytes":2097152,"elapsed_ms":410,"speed_bps":2557015}

=== Phase 4: Chunk 1 ===
[SENDER -> RECEIVER]
{"type":"CHUNK","transfer_id":"tid-001","chunk_index":1,"chunk_size":1048576,"chunk_checksum":"ddeeff..."}
<< 1,048,576 raw bytes >>

[RECEIVER -> SENDER]
{"type":"CHUNK_ACK","transfer_id":"tid-001","chunk_index":1}

=== Phase 5: Progress (final chunk) ===
[SENDER -> RECEIVER]
{"type":"PROGRESS","transfer_id":"tid-001","chunks_sent":2,"total_chunks":2,"bytes_sent":2097152,"total_bytes":2097152,"elapsed_ms":820,"speed_bps":2557015}

=== Phase 6: Completion ===
[SENDER -> RECEIVER]
{"type":"TRANSFER_DONE","transfer_id":"tid-001","file_name":"report.pdf","file_size":2097152,"checksum":"deadbeef..."}

  [Bob's device recomputes SHA-256 of saved file -> matches]

[RECEIVER -> SENDER]
{"type":"TRANSFER_COMPLETE","transfer_id":"tid-001","status":"success"}

[TCP CLOSE]
```

---

*End of LDFTP Specification v1.0*
