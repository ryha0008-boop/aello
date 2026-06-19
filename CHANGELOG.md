# Changelog

## [0.1.18]

### Fixed
- `aello login` now streams `claude setup-token` output live, so the auth URL is
  visible on headless machines (e.g. a VPS with no browser). Previously stdout was
  piped to capture the token, which swallowed the URL and made login appear to hang.
