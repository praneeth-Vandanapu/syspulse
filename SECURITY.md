<!--
  SysPulse - Multipurpose System Utility
  File   : SECURITY.md
  Author : Praneeth
  License: MIT
-->
# Security policy

## Supported versions

| Version | Supported |
|---|---|
| 0.7.x | Yes |
| < 0.7 | No |

## Reporting a vulnerability

Please **do not** open a public issue for security problems.

Use GitHub's private reporting instead: open the repository's **Security** tab and choose
**Report a vulnerability**. Include the SysPulse version, your Ubuntu version, the steps to
reproduce and what an attacker could achieve.

You can expect a first answer within 7 days. Fixed issues are listed in the release notes.

## Scope

Of particular interest:

* the privileged helper (`syspulse-helper`, started through `pkexec`) and its request validation;
* the packet-capture helper (`syspulse-capture`, file capabilities);
* App Launchers (files written into other users' accounts, folder ACLs);
* the web interface (anything that could make untrusted data execute in the app window);
* parsers of untrusted input: NetLens dissectors, pcap/pcapng files, speed-test and PacketForge servers.

Known limitations of the current release are listed in the README under **Security notes**.
