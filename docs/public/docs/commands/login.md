# `login` command

Authenticate with Proton Pass and establish a session.

## Synopsis

```bash
pass-cli login
pass-cli login --personal-access-token <TOKEN>
```

## Description

The `login` command authenticates you with Proton Pass. By default, it uses web-based authentication (prints a URL to complete the flow in your preferred browser). For automation, you can log in with a personal access token instead. Once authenticated, the session is stored locally and you can use other commands without re-authenticating.

## Web login

The easiest way to log into your Proton account in the Proton Pass CLI is to perform the login via web. In order to do so, run the following command:

```bash
pass-cli login
```

Then, open the URL that you will see in your web browser, complete the authentication flow, and your Proton Pass CLI will be automatically logged in.

The web login supports every Proton login flow, including SSO, two-factor authentication and hardware security keys.

### Web authentication flow

The login process follows these steps:

1. **Web authentication** - Open the printed URL and complete the login in your browser
2. **Extra password** (if required) - Proton Pass users can configure their accounts to require an additional Pass-specific password
3. **Initial setup** - The CLI performs first-time setup and creates a default vault named "Personal" if none exists
4. **Permission check** - Verifies that your account is authorized to use the CLI

### Extra password

Some Proton Pass accounts require an additional password (separate from your account password). This is the Pass-specific access password.

The CLI checks for it in this order:

1. **Environment variable** - Direct value
2. **File referenced by environment variable** - Path to file containing the value
3. **Interactive prompt** - If not found in env vars, prompts the user

**Interactive (default):**

If not supplied in any other way, after the web authentication step you'll be prompted for your Proton Pass extra password.

```bash
pass-cli login
# Enter Pass extra password:
```

**Via environment variable:**

```bash
export PROTON_PASS_EXTRA_PASSWORD='your-extra-password'
pass-cli login
```

**Via file:**

```bash
echo 'your-extra-password' > /secure/extra-password.txt
export PROTON_PASS_EXTRA_PASSWORD_FILE='/secure/extra-password.txt'
pass-cli login
```

You have 3 attempts to enter the correct extra password before the CLI logs out.

## Personal access token login

Personal access tokens allow logging in with a scoped credential instead of your full account. This is the recommended approach for CI pipelines, automated scripts, and any system where you want to limit what the session has access to.

You need to create a token first using the [`pat create`](personal-access-token.md) command, then grant it access to the relevant vaults or items.

**Via environment variable (recommended):**

```bash
PROTON_PASS_PERSONAL_ACCESS_TOKEN=pst_xxxx...xxxx::TOKENKEY pass-cli login
```

**Via command-line flag:**

```bash
pass-cli login --personal-access-token "pst_xxxx...xxxx::TOKENKEY"
```

Once logged in, the session works like any other. Run `pass-cli info` to verify it. It will show the token name under "Personal Access Token" instead of a user email. Note that PAT sessions have a lifetime of 2 hours and cannot have a session lock.

For more details on creating tokens and managing their access, see the [`pat` command reference](personal-access-token.md).

## Checking authentication status

After logging in, verify your session:

```bash
pass-cli info
```

This shows:

- Your username
- Account details
- Active session information

## Logout

To log out from your session, please check the [logout command](logout.md).

## Example: Automated login

For scripting and automation, use a personal access token:

```bash
#!/bin/bash

export PROTON_PASS_PERSONAL_ACCESS_TOKEN="$(cat ~/.proton-pass-token)"
pass-cli login
```

## Security considerations

- **Token storage**: Keep personal access tokens in a secret manager or a file with restrictive permissions, never in code
- **Least privilege**: Grant personal access tokens access only to the vaults and items they need
- **Session storage**: Authenticated sessions are stored locally and persist until logout
- **File permissions**: Ensure credential files have restrictive permissions (600)

## Supported authentication methods

- **Web login**: Supports every Proton login flow, including SSO, two-factor authentication and hardware security keys
- **Personal access token**: Scoped credential for CI and automation

## Session management

After successful login:

- Your session is stored in a platform-specific location
- The session persists across command invocations
- Use `pass-cli logout` to end the session
- Use `pass-cli info` to verify your session is valid

## Troubleshooting

### Login failures

- **Web login not completing**: Make sure you open the printed URL and finish the flow in your browser
- **Invalid or expired token**: Personal access token sessions last 2 hours; create a new token if it has expired or been revoked
- **Network issues**: Check your internet connection
- **Account locked**: Contact Proton support if your account is locked

### Environment variable issues

- **File not found**: Ensure the extra password file exists and is readable
- **Permission denied**: Check file permissions for credential files
- **Variable not set**: Verify environment variables are properly exported
