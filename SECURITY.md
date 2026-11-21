# Security

Report suspected vulnerabilities privately through the repository’s
[private vulnerability reporting form](https://github.com/cfcomputing/syncaidync/security/advisories/new).
Include the affected version, the platform, what happened, and the impact. Keep launch keys,
agent credentials, and private workspace data out of the report.

SyncAiDync runs on one machine for one user. Its security model aims to prevent other websites
and other local users from reaching the authenticated server. Agent CLIs run with the current
user’s permissions. Their own approval settings govern what they can do from the terminal.
