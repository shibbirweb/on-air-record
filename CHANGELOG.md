# Changelog

Every release of On Air Record, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and version numbers follow
[Semantic Versioning](https://semver.org/). Each release's section is also used as its notes on the
[releases page](https://github.com/shibbirweb/on-air-record/releases).

## [Unreleased]

### Added

- **See who is listening.** Point at the listener count in the top bar, or tap it on a phone, to see
  everyone connected right now: their account, what each browser tab is doing (not playing yet, live,
  paused, or listening back from a given time), the browser and device, the network address, and how long
  it has been connected. The list updates as people come and go, and a device that drops off the network
  without closing the page leaves it within about a minute. Only admins see it; without logins, everyone
  does, and people show as guests. (OAR-80)

### Changed

- **Changing someone's role on the settings page now waits for Save changes,** like every other setting
  there, instead of applying the moment it was picked from the menu. The account is marked Unsaved until you
  save, and Discard puts it back. Adding and removing accounts and setting passwords still happen as soon
  as you confirm them. (OAR-81)
- **The listener count includes people listening back through history,** not only those on the live feed,
  as the user guide always said it did. (OAR-80)

## [0.4.0] - 2026-09-26

### Added

- **Optional logins.** The first time anyone opens the page, it asks whether to set up accounts or keep
  the recorder open. With accounts, everybody signs in with an email and password. Admins control
  everything; listeners can listen, scrub back, change day and export, and the controls they cannot use
  are not shown to them. Admins add, change and remove accounts under Settings, Access, and everyone can
  change their own password under Account settings, in the account menu. An existing installation asks
  the question on its next visit, so open the page yourself straight after upgrading. (OAR-67)
- **Recovery from the host.** `on-air-record auth reset-password <email>` prints a new password for an
  account, and `on-air-record auth disable` switches logins off. Both work while the service is running.
  (OAR-67)
- **Two factor sign in.** Anyone with an account can add a 6 digit code from an authenticator app, such as
  Google Authenticator, Microsoft Authenticator, Authy or 1Password, as a second step after their password.
  Switch it on under Account settings by scanning a QR code. Ten one time recovery codes cover a lost phone;
  failing that, an admin can remove it from Settings, Access, and `on-air-record auth reset-2fa <email>`
  removes it on the host. (OAR-68)
- **Beta releases.** New features now reach a beta first, a pre-release like `0.4.0-beta.1`, before the
  stable release. Install or switch to betas with the installer's `--beta` option (`-Beta` on Windows). The
  choice is remembered, so later updates stay on betas; `--stable` goes back, and never to a version older
  than the one installed. Anybody who does not ask for betas keeps getting stable releases only. (OAR-69)

### Fixed

- The installers no longer hang when run with no terminal to answer, such as from a script, while the
  default port 8080 is already in use. They stop at once and say to pass `--port` (`-Port` on Windows).
  (OAR-70)

### Security

- Other websites can no longer control the recorder or listen to it through a visitor's browser, with or
  without accounts. Previously any page opened on the same network could start or stop recording, change
  settings, delete recordings through a shorter retention window, download audio, and listen live.
  (OAR-67)
- Removing an account, changing a password or signing out now ends that person's live stream within 15
  seconds, not only their next page load. (OAR-67)
- After five wrong passwords, a device must wait 15 minutes before trying again. (OAR-67) Wrong two factor
  codes count towards the same limit, and a code cannot be used twice. (OAR-68)

## [0.3.0] - 2026-09-26

### Added

- **Start up delay for automatic recording.** A new setting under "Record on start up" makes the service
  wait a number of seconds before it starts recording. Use it when a machine that starts On Air Record at
  boot records silence after a reboot, and pressing Stop then Start fixes it: the microphone, usually a USB
  one, was not ready yet, so recording started on the built in input instead. Ten to thirty seconds is
  usually enough. The web page is available straight away; only the recording waits. The default is no
  delay, so nothing changes unless you set it. (OAR-62)
- **A way to stop an installed copy.** The installers now write a stop script beside the start script:
  `stop.sh` on macOS and Linux, `stop.cmd` and `stop.ps1` on Windows. On macOS and Linux it asks the
  service to shut down and waits, so the recording in progress is saved. Windows has no gentle way to stop
  a console program, so there it is an immediate stop. It only stops the copy in its own folder, so a
  second installation elsewhere keeps running. (OAR-60)

### Documentation

- The installation guide has a new "Stopping it" section, and troubleshooting entries for a service left
  running after its terminal closed and for silent recordings after a reboot. (OAR-60, OAR-62)
- Diagrams in the README, the installation guide and the developer docs. (OAR-61)
- **Linux microphone access.** A new section of the installation guide explains why the app cannot record
  from an account that is not in the `audio` group, which is usual for a login over SSH, and how to fix it.
  It also explains why running it with `sudo` instead is a trap: files created as `root` later stop it
  saving anything while it still shows Recording. It gives the steps to recover, including for the
  systemd service, and how to run it under a separate account instead of your own or `root`. The README
  and the troubleshooting list point to it. (OAR-66)

## [0.2.0] - 2026-09-05

### Added

- **One command installer for macOS and Linux.**
  `curl -fsSL https://raw.githubusercontent.com/shibbirweb/on-air-record/master/scripts/install.sh | sh`
  picks the right build for the machine, downloads the latest release, checks it against the published
  checksum, asks once which port to use, and starts the service. Everything lives in one `on-air-record`
  folder with a `start.sh` that remembers your settings. Re-run it with `--update` for a newer release,
  `--release` for a specific one, or `--reconfigure` to change the port. (OAR-49)
- **PowerShell installer for Windows.**
  `irm https://raw.githubusercontent.com/shibbirweb/on-air-record/master/scripts/install.ps1 | iex` does the
  same, with a `start.cmd` that can be double clicked. (OAR-51)

### Changed

- The footer link now reads "Star the repository", and its separators no longer dangle when the footer
  wraps on a phone. (OAR-55)
- The README opens with a screenshot of the control room and the quick start. (OAR-48, OAR-50)

### Development

- CI runs both installers on macOS, Linux and Windows, and every release is verified by installing it on
  all three. (OAR-52, OAR-53)
- `node scripts/version.mjs bump` and `pending` say when a release is due and what it should be numbered.
  (OAR-56)
- A `Makefile` wraps the everyday commands. (OAR-57)

## [0.1.0] - 2026-09-05

The first release.

### Added

- **Always on recording.** Captures from any input device on the host and writes it to disk in fixed
  length segments indexed in SQLite, whether or not anyone is listening. Recording can start
  automatically when the service starts, and the input device can be changed without a restart.
- **Live broadcast.** Every browser on the local network hears the same live signal over a WebSocket,
  with a level meter and adjustable input gain.
- **DVR timeline.** A CCTV style timeline with a waveform. Click anywhere to play from that moment, zoom
  around the marker, move the window with a 24 hour minimap, and jump back to live with one click.
  Recordings are organised by day, and a calendar picks a day while disabling days with no audio.
- **Variable speed playback** from a quarter to four times real time.
- **Bookmarks** with flags and a jump list. They are removed along with the audio they point at.
- **WAV export** of any time range, chosen on the timeline or typed as start and end times.
- **Storage control.** A retention window, or keep recordings forever. A custom recordings directory with
  a button to test it. A choice of recording sample rate to trade quality for disk space, with a live
  estimate of how much space the settings will use.
- **Settings page** where edits are held until you press Save, and a reset that warns when it would delete
  audio.
- **Single file releases.** The web interface is built into the program, so a release is one file to run.
  Archives for macOS (Apple silicon and Intel), Linux x86_64 and Windows x86_64, each with a checksum,
  plus a systemd unit for running it as a Linux service.
- **Guides.** A user guide with screenshots of every feature and an installation guide for all three
  platforms, also published to the project wiki.

### Known limitations

- The Windows build is built and tested in CI only, and has not yet been run on a Windows desktop.
- The last few seconds of live audio cannot be scrubbed back to until the segment being written closes.
- There is no authentication, by design. Anyone who can reach the port can listen, so keep it on a network
  you trust.

[0.4.0]: https://github.com/shibbirweb/on-air-record/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/shibbirweb/on-air-record/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/shibbirweb/on-air-record/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/shibbirweb/on-air-record/releases/tag/v0.1.0
