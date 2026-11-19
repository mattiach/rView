# Security policy

## Reporting a vulnerability

Report it privately through GitHub: <https://github.com/mattiach/rview/security/advisories/new>
(repository **Security** tab → **Report a vulnerability**). Please do not open a public issue for it.

Include the rView version (shown in the empty window, or the release you downloaded), the Windows version, the steps
to reproduce and, when a file triggers it, that file or a way to produce it. You will get an answer in the
advisory; a fix is released as a new version, and the advisory is published once it is out.

## Supported versions

Only the latest release gets security fixes.

## Scope

In scope: anything a crafted input can do to rView — image files, `.rvm` projects, the clipboard, Open URL
downloads, the update check — such as crashes, hangs, excessive memory use, writes outside the chosen file,
or running code; and the installer.

Not vulnerabilities, by design (documented in the README):

- Blur and Pixelate redactions are visual effects; only Solid hides data.
- A `.rvm` project contains the whole original image, including what is under its redactions.
