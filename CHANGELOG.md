# Changelog

Every release of On Air Record, newest first. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and version numbers follow
[Semantic Versioning](https://semver.org/). Each release's section is also used as its notes on the
[releases page](https://github.com/shibbirweb/on-air-record/releases).

## [Unreleased]

### Added

- **The whole compose file, to copy.** The README, the setup guide and the Docker Hub page now show the
  complete `compose.yaml` with a note on each line, and an example `.env`, so it can be written by hand
  instead of downloaded. (OAR-105)
- **Jump between sounds.** New next and previous sound buttons beside the 30 second rewind jump straight
  to the moments something was heard, starting a second early so each is heard from its beginning, and
  those moments are marked in teal on the timeline and the day overview. What counts as a sound is judged
  against the room's own background, so it suits a quiet bedroom and a noisy kitchen alike; admins can
  make it more or less sensitive under Settings, Finding sounds. (OAR-107)

- **Prometheus metrics.** The recorder now reports its state at `/api/metrics` for Prometheus: whether
  it is recording, whether writing to disk works, the microphone level, disk use, how far back the
  recordings reach and who is listening. Settings, Monitoring has a scrape config to copy and, for a
  recorder with logins, a scrape token that lets Prometheus read the metrics and nothing else. The setup
  guide suggests three alerts for an unattended recorder. (OAR-110)

### Fixed

- **Playback faster or slower than real time is smooth.** At double speed each moment of audio was
  followed by a moment of silence and the audio skipped now and then, and the playhead moved at normal
  speed whatever the speed was set to. Both now keep to the chosen speed. (OAR-108)
- Jumping to the end of the recordings while playing faster than normal joins the live feed at normal
  speed. Before, the next jump back into the recordings played too fast and skipped. After a network
  hiccup during faster or slower playback, the audio also no longer arrives in a rush. (OAR-108)
- Jumping past the end of the recordings while nothing is recording shows playback as stopped, as it
  already did when playback reached the end on its own. (OAR-108)
- Raising the volume while muted turns the sound back on, as the mute button already showed. Before, the
  button said the sound was on while playback stayed silent. (OAR-108)
- Restore defaults puts the recording sample rate back as well. (OAR-108)
- When removing an account or resetting its two factor sign in is refused, the reason stays on screen
  instead of disappearing at once. (OAR-108)
- A recordings folder with spaces in its name can be typed as it is; the spaces were being removed while
  typing. (OAR-108)
- The success message after changing a password goes away when the next attempt fails, rather than
  showing beside the error. (OAR-108)
- While recording at a lower bit rate, such as Telephone, the bit rate list in Settings offers every rate
  the microphone can give again, not only Match the device and the rate in use; and the disk estimate for
  Match the device uses the microphone's own rate. (OAR-108)
- On Windows, the service no longer crashes some time after the list of microphones was shown, or when
  recording starts after it was. (OAR-108)
- On a phone the control room fits the screen. The row of zoom buttons above the timeline did not wrap,
  so the whole page was a little wider than the phone and opened slightly zoomed out. (OAR-108)
- The waveform on the Broadcast card shows what you are hearing again. It was drawn at true size, so a
  microphone at a normal speaking level left it looking empty; quiet sound is now enlarged to fill the
  box, loud sound is never cut off, and silence shows as a flat line. (OAR-108)
- The level meter on the recorder card keeps showing the microphone while you play back a recording or
  pause. Before, it froze on its last reading as soon as you clicked the timeline. (OAR-108)
- The storage panel says Forever for recordings kept forever, instead of 0 h. (OAR-108)
- Where the clocks change at midnight, as in the Azores, Cuba and Chile, the day of the change and the day
  before it no longer share or lose an hour of recording in the day picker, and such a day is no longer
  missing from it. (OAR-108)
- With a sound card that hands over audio in chunks of half a second or more, the recording no longer
  splits into short segments with small jumps in time between them. (OAR-108)
- Sizes just under a unit read as the next unit up, such as 1.0 MB rather than 1024.0 KB. (OAR-108)
- On Windows, the update command shown for an install in a folder whose name has a curly apostrophe, such
  as one typed on a phone, now works when pasted into PowerShell. (OAR-108)
- The level meter no longer stays red after recording stops. (OAR-108)
- Hint and secondary text in the light theme is a shade darker, so it reads clearly against its
  background. (OAR-108)
- Screen readers now announce the volume, gain, segment length and start up delay sliders, the on air
  sign and the bookmark count by name. (OAR-108)
- **Live audio no longer pauses when storage is slow.** Another program holding the database, such as a
  backup, used to pause live audio for everyone for up to five seconds each time a recording segment
  finished, and a disk that stopped responding could stop it altogether. Saving now runs on its own, with
  a minute of room to catch up, so listeners hear the live feed without a break; if storage stays stuck
  longer than that, only the recording misses audio, and the recorder panel says so. (OAR-108)
- **The recorder panel says when recordings stop reaching the disk.** If the recordings folder cannot be
  written, the disk is full, or the database will not take new recordings, the live feed carries on as
  before and the panel now says why nothing is being saved, clearing by itself once the problem is fixed.
  Before, it went on looking like a healthy recorder. (OAR-108)
- A recording finished while another program, such as a backup, was holding the database is added to the
  timeline once the database is free, instead of being lost from it. (OAR-108)
- When a recording file has gone missing or been cut short, playback reports the gap and an export has
  silence in its place, so everything after it stays at the right time. Before, an export moved the rest
  of the audio earlier, and a long run of missing files ended playback early or silenced the rest of an
  export. (OAR-108)
- After a moment of audio that could not be saved, the recording carries on at the right time. Before,
  the rest of that stretch played one frame early. (OAR-108)
- A recording whose folder was deleted while it was being written no longer shows on the timeline as
  audio that cannot be played. (OAR-108)
- The clean up of old recordings keeps going past files it cannot delete. Before, a few hundred such files
  stopped it deleting anything newer, and the disk filled. (OAR-108)
- When the service cannot start because of a folder or the database, the message names the file or folder
  to fix, and a database file it cannot write to is refused at start instead of running without saving
  anything. (OAR-108)

## [0.8.1] - 2026-09-27

### Added

- **A page on Docker Hub.** The image's page there, `shibbirweb/on-air-record`, now describes it instead of
  standing blank: what it does, the Docker Compose and `docker run` quick starts, what each tag means, how
  the container gets the microphone, and every environment variable. From now on it is kept up to date
  whenever the instructions change. (OAR-101)

### Changed

- **Docker is now the recommended way to run it on Linux,** and the README and setup guide lead with it
  there. It takes away what usually goes wrong with a direct install: there is no audio group to join and
  no `sudo` mistake that leaves recordings it cannot save, it starts again with the machine without a
  service to set up, it updates with two commands, and a Raspberry Pi on a 64 bit OS gets a ready made
  build. The installer stays the way on macOS and Windows. The setup guide also lists every environment
  variable the container reads, and which ones to leave alone. (OAR-98)

## [0.8.0] - 2026-09-27

### Added

- **Docker image for Linux.** Each release is now also published as a container image, on
  `ghcr.io/shibbirweb/on-air-record` and on Docker Hub as `shibbirweb/on-air-record`, for 64 bit Intel, AMD
  and ARM machines, so a Raspberry Pi with a 64 bit OS no longer has to build from source. Download
  `packaging/compose.yaml`, write the host's audio group and time zone into a `.env` file beside it, and
  `docker compose up -d`; the host's sound cards are passed through and the microphone is picked on the
  settings page as usual. It needs a Linux host: Docker Desktop on macOS and Windows cannot give a container
  the microphone. Update notices in a container show the commands to pull the new image, and `docker ps`
  shows whether it is healthy. When the container cannot reach the microphone, the log and the recorder
  panel say why and what to change, such as the exact group number to set. The setup guide has the
  details. (OAR-92)

## [0.7.0] - 2026-09-26

### Added

- **Update notices.** When a new version is released, admins see a note under the top bar with what changed
  in every release since theirs and the exact steps to update for how it was installed. The recorder never
  updates itself. The Updates card in Settings shows the running version and has a Check now button. The
  check asks GitHub every six hours and is the only request the recorder makes to the internet; switch it
  off there if the machine should make none. (OAR-87)

## [0.6.0] - 2026-09-26

### Added

- **Keeps playing with the screen off.** On a phone, the broadcast now carries on when the screen turns off
  or you switch apps, like a radio app, and shows on the lock screen with play and pause buttons. Before,
  locking the phone stopped the sound. On a computer it now shows in the browser's media controls and the
  system's Now Playing, and the keyboard's play and pause key works. (OAR-84)

## [0.5.0] - 2026-09-26

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

[0.8.1]: https://github.com/shibbirweb/on-air-record/compare/v0.8.0...v0.8.1
[0.8.0]: https://github.com/shibbirweb/on-air-record/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/shibbirweb/on-air-record/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/shibbirweb/on-air-record/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/shibbirweb/on-air-record/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/shibbirweb/on-air-record/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/shibbirweb/on-air-record/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/shibbirweb/on-air-record/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/shibbirweb/on-air-record/releases/tag/v0.1.0
