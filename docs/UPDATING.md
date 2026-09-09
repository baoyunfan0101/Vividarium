# Application Update Backend API

The desktop backend checks and installs signed Vividarium releases from:

```text
https://github.com/baoyunfan0101/Vividarium/releases/latest/download/latest.json
```

The frontend uses the commands below. Direct access to the Tauri updater plugin is
not exposed through the desktop capability.

## Models

### `AppUpdateInfo`

Returned by `check_app_update` when a newer compatible release exists.

| Field | Type | Description |
| --- | --- | --- |
| `current_version` | `string` | Version of the running application. |
| `version` | `string` | Version offered by the release endpoint. |
| `notes` | `string \| null` | Optional release notes. |
| `published_at` | `string \| null` | Optional release publication timestamp. |

### `AppUpdateEvent`

Sent through the `onEvent` channel while an update is installed. The serialized
value is tagged by `event`.

| Event | Data | Description |
| --- | --- | --- |
| `started` | `{ content_length: number \| null }` | Download started. The total size may be unavailable. |
| `progress` | `{ chunk_length: number, downloaded: number }` | A chunk was downloaded and the cumulative byte count changed. |
| `finished` | none | The update package was downloaded and installed. |

## Commands

### `get_app_version`

Parameters: none.

Returns: `string`, the running package version.

### `check_app_update`

Parameters: none.

Returns: `AppUpdateInfo | null`. A non-null result is retained by the backend for
the next `install_app_update` call. A null result clears any previously retained
update.

The command only checks release metadata. It does not download or install the
package.

### `install_app_update`

Parameters:

| Parameter | Type | Description |
| --- | --- | --- |
| `onEvent` | Tauri `Channel<AppUpdateEvent>` | Receives download and installation progress. |

Returns: `null` on completion. The application restarts after a successful
installation. If installation fails, the retained update remains available for a
retry.

Call `check_app_update` and receive a non-null result before calling this command.
Installation is rejected while any backend operation is running. Update checks
remain available during backend operations.

## Data Compatibility

Vividarium 3.0.0 uses SQLite schema version `2`. Vividarium 3.1.0
automatically upgrades supported schema-2 metadata, taxonomy, and Photo Library
databases directly to schema `3`; fresh databases use schema `3`. Other schema
versions return an incompatibility error. The schema upgrade is forward-only:
databases opened by 3.1.0 are not supported by Vividarium 3.0.0.

## Release Requirements

Every update artifact is signed. The public verification key is bundled in the
desktop configuration. GitHub Actions generates `latest.json`, platform update
artifacts, and their signatures from the `TAURI_SIGNING_PRIVATE_KEY` repository
secret and its `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
