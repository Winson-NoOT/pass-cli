# `export` command

Export your Proton Pass data to portable formats you can keep outside the app.

## Synopsis

```bash
pass-cli export <SUBCOMMAND>
```

## Description

The `export` command takes your decrypted data and writes it to a file you control. Each subcommand targets a different output format, so you can pick the one that fits your backup, migration, or archival needs.

Exported files are not protected by Proton's end-to-end encryption. Store them somewhere safe and delete them once you no longer need them.

## Subcommands

### archive

Export vaults, and items shared with you, to a self-contained offline HTML archive.

```bash
pass-cli export archive [OPTIONS]
```

The resulting file embeds everything it needs, so it opens in any browser without network access or a Proton Pass installation.

**Options:**

- `--vault-name NAME` - Vault to export. Repeatable. When omitted, all vaults are exported together with items shared directly with you
- `--include-attachments` - Download and embed file attachments. Increases the file size
- `--encrypt` - Encrypt the archive with a password using AES-256-GCM and Argon2id
- `--password-env VAR` - Read the encryption password from the environment variable `VAR` instead of prompting. Requires `--encrypt`
- `-o, --output PATH` - Output path for the HTML file. Defaults to `proton-pass-archive.html`

On Unix systems the file is created with `0600` permissions.

**Examples:**

```bash
# Export everything
pass-cli export archive

# Export a single vault with attachments
pass-cli export archive --vault-name "Work" --include-attachments --output work.html

# Export two vaults, encrypted with a password from the environment
export ARCHIVE_PASSWORD="my strong password"
pass-cli export archive --vault-name "Work" --vault-name "Personal" \
    --encrypt --password-env ARCHIVE_PASSWORD --output backup.html
```

When `--encrypt` is used without `--password-env`, the CLI prompts for the password twice. Opening an encrypted archive in the browser asks for the same password before rendering anything.

**Notes:**

- Items are sorted alphabetically by title in the archive
- Trashed items are included and marked as such
- Failed attachment downloads are skipped with a warning, the rest of the archive is still written
- If a vault name cannot be resolved, the command fails before writing anything

## Related commands

- **[vault](vault.md)** - List vaults and their names
- **[item](item.md)** - Inspect individual items
