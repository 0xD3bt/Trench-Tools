# LaunchDeck Metadata And Vanity Mints

This page explains how LaunchDeck handles token metadata/IPFS uploads and local vanity mint queues for each launchpad.

## Metadata And IPFS

LaunchDeck treats metadata as launch input. If the form already has a metadata URI, LaunchDeck uses it and does not upload again. Gateway-style IPFS URLs such as `https://ipfs.io/ipfs/<cid>` are normalized to `ipfs://<cid>`. Non-IPFS HTTP URLs are left as-is.

If no metadata URI is present:

- `Pump` uses the shared LaunchDeck metadata uploader. The default uploader posts the local image and token fields to pump-fun and uses the returned `metadataUri`.
- `Bonk` uses the shared LaunchDeck metadata uploader. The default uploader uploads the image to Bonk, then uploads metadata JSON to Bonk and uses the returned URI.
- `Bagsapp` owns its metadata flow through the Bags API. LaunchDeck does not pre-upload through the shared Pump/Bonk uploader. During Bags prepare, Bags uploads token info and image, then returns the mint, config key, and metadata URI used for the launch transaction.

You can override the Pump/Bonk uploader with Pinata:

```bash
LAUNCHDECK_METADATA_UPLOAD_PROVIDER=pinata
PINATA_JWT=YOUR_PINATA_JWT
```

When Pinata is selected, LaunchDeck uploads the image to Pinata, builds metadata JSON with that image URI, uploads the metadata JSON, and uses the returned `ipfs://` URI. If Pinata fails, LaunchDeck falls back to the launchpad default uploader and returns a warning with the response. If both Pinata and the default uploader fail, the launch request fails.

The shared metadata JSON includes:

- `name`
- `symbol`
- `description`
- `image`
- `createdOn` (`https://pump.fun` for Pump, `https://bonk.fun` for Bonk)
- optional `website`, `twitter`, and `telegram`

Pump metadata also includes `showName: true`. Bonk metadata follows the Bonk upload shape.

## Image Selection And Cropping

The LaunchDeck image field opens the local image library and can crop the selected image before upload. Use `Crop` after selecting an image to snip a region or use pan mode for a square crop; saving the crop creates a new local image-library entry and uses that cropped file for the current launch.

When LaunchDeck opens from J7 tweet context, it can show detected tweet image candidates near the image field. Select a candidate to use it for the current launch, crop it if needed, or save it into the local image library for reuse.

## Vanity Mint Queues

LaunchDeck supports local file-backed vanity mint queues for:

- `Pump`
- `Bonk`

`Bagsapp` does not use a vanity queue because Bags already returns the launch mint during its prepare flow.

The queue files live under local runtime state:

```text
.local/trench-tools/vanity/pump.txt
.local/trench-tools/vanity/bonk.txt
```

LaunchDeck creates template files automatically if they are missing. Empty files are valid and do not warn.

## Mint Selection Order

For Pump and Bonk launches, mint selection is:

1. Use the explicit per-launch `vanityPrivateKey` if the launch form provides one.
2. Otherwise reserve the next valid local queued key from `pump.txt` or `bonk.txt`.
3. Otherwise generate a random mint keypair.

The queue is fail-open when it is empty or has no valid available key. In that case, LaunchDeck does not block the launch; it falls back to a random mint. If LaunchDeck cannot complete an RPC availability check for a queued key it is trying to reserve, treat that as an operational warning and verify RPC health before relying on the queue.

Explicit `vanityPrivateKey` entries are checked against chain before use. Queued vanity mints are parsed and checked before reservation, then marked as used when a send attempt begins. They are not consumed during ordinary preview/build work.

## Correct Queue Format

Each active line can be just one private key. Use exactly one base58-encoded 64-byte Solana keypair string per line. This is the full keypair/private key encoded as base58, not a public address and not a JSON byte array.

No commas, brackets, quotes, or labels are required.

Correct shape:

```text
<base58_64_byte_keypair_private_key>
<base58_64_byte_keypair_private_key>
<base58_64_byte_keypair_private_key>
```

Optional comments are allowed after the private key. The part after `#` is ignored only when there is whitespace before the `#`.

```text
<base58_64_byte_keypair_private_key> # optional derived public mint address
```

Accepted:

```text
# Pump vanity queue
BASE58_64_BYTE_PRIVATE_KEY_FOR_MINT_ENDING_INpump
BASE58_64_BYTE_PRIVATE_KEY_FOR_ANOTHER_MINT_ENDING_INpump
BASE58_64_BYTE_PRIVATE_KEY_FOR_THIRD_MINT_ENDING_INpump # optional note
```

Rejected:

```text
[1,2,3,...]
"BASE58_64_BYTE_PRIVATE_KEY"
BASE58_KEYPAIR_ONE,BASE58_KEYPAIR_TWO
BASE64_SECRET_KEY
twelve word seed phrase ...
PUBLIC_MINT_ADDRESS_ONLY
BASE58_KEYPAIR_WITH_SPACES_INSIDE
```

Suffix rules:

- `pump.txt`: derived public mint address must end with `pump`.
- `bonk.txt`: derived public mint address must end with `bonk`.

The suffix check is case-sensitive because Solana addresses are base58 strings. Generate the key for the exact suffix expected by the target launchpad.

## Validation And Diagnostics

On startup, LaunchDeck reads `pump.txt` and `bonk.txt`, checks formatting, rejects duplicates, and reports diagnostics without printing private keys. Incorrect lines show as `WARN vanity-pool` diagnostics in the normal `npm start` console. Runtime status and extension diagnostics can also surface vanity queue state. Empty queue files do not warn.

LaunchDeck does not auto-correct queue files. Invalid formats are skipped so the operator can fix the source file intentionally.

During background refresh, LaunchDeck can use RPC to check whether queued public mint accounts already exist on-chain. Already-used queued mints are marked `on-chain-used` and removed from the active queue file.

Consumed and diagnostic state is written separately:

```text
.local/trench-tools/vanity/pump.used.jsonl
.local/trench-tools/vanity/bonk.used.jsonl
```

The used state stores public keys, redacted key hashes, reservation IDs, statuses, and optional signatures. It does not store private keys.

Keep vanity queue files local. Do not commit `.local/trench-tools/vanity/*.txt` or share queue contents in screenshots, logs, Discord, or issues.
