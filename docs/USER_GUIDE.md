<!--
  This file is published to the GitHub wiki by .github/workflows/wiki.yml on every push to master, which
  rewrites the links between documents on the way. Images are linked by absolute raw.githubusercontent URL
  rather than by relative path, because the wiki is a separate repository and cannot see docs/images.
-->

# User guide

On Air Record turns any computer with a microphone into a small radio station with a rewind button.

It does two things at the same time, all day, without being asked:

- **It broadcasts.** Whatever the microphone hears is sent to any web browser on your network, live.
- **It records.** Everything is saved to the computer's disk, so you can go back and listen to any moment
  from the past few hours or days.

That combination is what makes it useful. If something happens in the room and nobody was listening at the
time, you can still go back and hear it, the same way you would rewind a security camera.

You do not need to install anything to listen. There is no account, no password, and no app. You open a
web address in an ordinary browser.

If nothing is running yet, start with the **[installation guide](SETUP.md)**, which covers downloading and
running it on macOS, Linux and Windows.

> The screenshots in this guide come from a demonstration setup with several days of example recordings
> already on disk, so the timeline looks busy. A brand new installation starts empty and fills up as it
> records.

## Contents

- [Opening it](#opening-it)
- [The screen at a glance](#the-screen-at-a-glance)
- [The top bar](#the-top-bar)
- [Listening to what is happening now](#listening-to-what-is-happening-now)
- [Choosing which microphone to use](#choosing-which-microphone-to-use)
- [The timeline](#the-timeline)
- [Going back to an earlier moment](#going-back-to-an-earlier-moment)
- [Playing slower or faster](#playing-slower-or-faster)
- [Jumping to another day](#jumping-to-another-day)
- [Bookmarks: naming a moment](#bookmarks-naming-a-moment)
- [Saving a piece of audio as a file](#saving-a-piece-of-audio-as-a-file)
- [Checking that it is still recording](#checking-that-it-is-still-recording)
- [Disk space](#disk-space)
- [Settings](#settings)
- [On a phone or a tablet](#on-a-phone-or-a-tablet)
- [Dark mode](#dark-mode)
- [Questions and problems](#questions-and-problems)
- [Reporting a problem](#reporting-a-problem)
- [Credits](#credits)

## Opening it

Two useful terms before we start:

- **The host computer** is the machine the microphone is plugged into, running the On Air Record program.
  It does the recording. It has to stay switched on.
- **Your browser** is wherever you are listening from. That can be the host computer itself, or a laptop,
  phone or tablet on the same network.

On the host computer, open:

```
http://localhost:8080
```

From any other device on the same network, use the host computer's address on the network instead. Whoever
set the service up can tell you what it is. It looks something like:

```
http://192.168.1.24:8080
```

The `8080` on the end is the port. 8080 is only the default and whoever installed it may have chosen
another number, in which case your address ends in that number instead. Either way the port has to be
included, because a browser assumes a different one if you leave it out. Changing it is covered in the
[installation guide](SETUP.md#choosing-a-port).

Anyone who can open that address can listen and can change the settings. There is no login. This is
intended for a network you trust, such as an office or a home, and not for the open internet.

## The screen at a glance

Everything you need is on one page, called the control room.

![The control room, with the broadcast panel and timeline on the left and the recorder, source and storage panels on the right](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/control-room.png)

The wide left column is what you use minute to minute:

- **Broadcast** is the play button and the volume.
- **Timeline** is the recording history, and how you travel back through it.

The narrow right column is what you glance at occasionally:

- **Recorder** shows whether it is still recording and how loud the microphone is.
- **Source** is which microphone it is listening to.
- **Storage** is how much disk has been used.

Along the bottom of every page is a strip with the licence, a link to the project on GitHub, a link for
reporting a problem, and the version number.

## The top bar

![The top bar, showing the ON AIR sign, the page links, the connection status and the listener count](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/header.png)

Reading from left to right:

- **Control room** and **Settings** switch between the two pages. Switching pages does not interrupt what
  you are listening to.
- **ON AIR** lights up red only when all three things are true: the computer is recording, you have pressed
  play, and you are listening to the live feed rather than to something from the past. It is a quick way to
  answer "am I hearing the room right now?".
- **Stream connected** means your browser is talking to the host computer. If it says disconnected, see
  [Questions and problems](#questions-and-problems).
- **The listener count** is how many browsers are currently connected.
- **The moon or sun icon** switches between light and dark colours.

## Listening to what is happening now

![The broadcast panel, showing the live waveform and the transport controls underneath](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/broadcast.png)

Press the orange play button and you will hear the microphone.

You have to press it. Web browsers refuse to let any page start making noise on its own, so nothing plays
until you click. This is a browser rule, not a limitation of On Air Record.

The box above the buttons draws the sound as it arrives, so you can see that audio really is coming through
even with the volume turned down.

![The transport bar: play, back thirty seconds, go live, the clock, the on air badge, the speed control and the volume slider](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/transport.png)

The row of controls, left to right:

| Control | What it does |
| --- | --- |
| Play and pause | Starts and stops your listening. It does **not** stop the recording. |
| Back 30 seconds | Jumps back half a minute. Useful when you half heard something. |
| Go live | Returns you to the present from anywhere in the past. |
| The clock | The time of the audio you are hearing at this instant. |
| on air | A reminder that you are hearing the present, not a recording. |
| Speed | Plays recordings slower or faster. Only works on recordings, not on live audio. |
| Volume | Your own volume. It changes nothing for anyone else and nothing on the recording. |

Pausing only pauses **you**. The host computer carries on recording the whole time, so nothing is lost while
you are away.

## Choosing which microphone to use

![The source panel, showing the selected input and which microphone is being captured](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/source.png)

The source panel names the microphone currently being recorded. **System default** means "whichever
microphone the computer itself is set to use", which is usually what you want.

To pick a specific one, open the list:

![The input source list, showing system default, the built in microphone and another audio device](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/source-open.png)

Choosing a different microphone restarts the recording immediately, under the new microphone. Everything
recorded before that stays exactly where it is and remains playable.

If you plug in a new microphone while the page is open, it will not appear in the list straight away. Press
the circular arrows button next to "Input source" to look again.

## The timeline

This is the part worth understanding properly, because everything else follows from it.

![The timeline, with the toolbar, the main scrubber showing four hours, and the day overview bar underneath](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/timeline.png)

There are two bars, and they show different things.

**The big bar in the middle** is the part of the day you are looking at closely. In the picture above it
covers four hours. The orange shape is the sound itself: tall where it was loud, flat where it was quiet.
The pale shaded background marks the stretches that were actually recorded. Where the background is plain
white, nothing was recorded, usually because the service was stopped or the computer was asleep.

**The thin bar underneath** is the whole day, midnight to midnight, always. The small rectangle sitting on
it shows which slice of the day the big bar is currently showing. Drag that rectangle and the big bar
follows, which is the fastest way to move a long way at once.

Two coloured lines can appear on the big bar:

- **A red line** is the present moment, the live edge. Nothing exists to the right of it yet.
- **A black line** is your cue marker, the point you last clicked.

The buttons above the bars:

| Button | What it does |
| --- | --- |
| Today | Opens a calendar for picking a different day. |
| The bookmark with a plus | Saves a name for the moment you are listening to. |
| The bookmark with a number | Lists the bookmarks you have saved. |
| The download arrow | Saves part of the recording as a file. |
| The frame icon | Resets the view to a sensible standard zoom. |
| Magnifying glasses | Zoom in and out. |
| 1m to 24h | Jump straight to a window of that length. |
| Following live | When switched on, the view slides along on its own to keep up with the present. |

To move around by hand: click to place the cue marker and start playing from there, drag left or right to
pan, and scroll to zoom in and out. Zooming always keeps your cue marker in view, so you can zoom right in
on a moment without losing it.

![The timeline zoomed in to one minute, with individual seconds visible](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/timeline-zoomed.png)

Zoomed all the way in to one minute you can see individual seconds, which is close enough to find one
particular noise.

## Going back to an earlier moment

Click anywhere on a shaded part of the timeline. Playback jumps there and carries on from that point.

![The transport bar while playing back a recording, showing how far behind live it is](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/transport-playback.png)

Once you are listening to the past, the controls change to tell you so. The "on air" badge is replaced by
how far behind the present you are, "4m 14s behind" in the picture above, and the clock shows the time of
the recording you are hearing rather than the time now.

Press **Go live** to jump back to the present.

If you simply keep listening, the recording will eventually catch up with the present by itself, and the
service will hand you over to the live feed without a gap.

One thing that surprises people: **the last few seconds of live audio cannot be rewound to yet.** Audio is
written to disk in short blocks, and a block only becomes rewindable once it has been finished and closed.
The most recent seconds are audible live but are not yet on the timeline. They appear a moment later.

## Playing slower or faster

![The playback speed menu, offering a quarter speed up to four times speed](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/speed.png)

While you are listening to a recording, the speed control lets you slow down to a quarter speed to catch a
mumbled word, or speed up to four times to skim a long quiet stretch.

The control is greyed out while you are listening live, because you cannot play the present faster than it
is happening.

## Jumping to another day

![The calendar, with only the days that have recordings selectable](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/day-picker.png)

Click **Today** to open the calendar.

Only days that actually have recordings can be clicked. Everything else is greyed out, so you cannot land
on an empty day by mistake. Underneath, the calendar tells you what that day holds: the first and last
recording, how much was recorded in total, and how much disk it takes.

Days disappear from this calendar once they pass out of the retention window, because the recordings
themselves have been deleted by then. See [Settings](#settings) to change how far back it keeps.

## Bookmarks: naming a moment

A bookmark is a name pinned to a point in time, so you can find your way back to it later without
remembering when it happened.

![The bookmark form, with a label being typed for the current moment](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/bookmark-add.png)

Press the bookmark with a plus. The moment it saves is the moment you are listening to, not the middle of
the screen, so you can hear something, press the button, and describe what you just heard. Type a short
label and press Save.

![The bookmark list, with the newest first and a delete button on each](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/bookmark-list.png)

The button next to it shows how many bookmarks exist. Open it and click any one to jump straight there.
Newest are at the top, since the one you just made is usually the one you want back.

Bookmarks also show up as small flags on both timeline bars, so you can see them while you scroll.

A bookmark is deleted along with the audio around it when that audio passes out of the retention window.
The name on its own would not be much use once the sound is gone.

## Saving a piece of audio as a file

![The export panel, with a fifteen minute range chosen and the resulting file size](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/export.png)

Press the download arrow to save a stretch of recording as a WAV file, which every computer and phone can
play and every audio program can open.

You can choose the range in three ways:

- **Visible window** takes exactly what the timeline is showing.
- **Last 1m, 5m, 15m** take the most recent minute, five minutes or quarter hour.
- **From and To** let you type any two times you like.

Before you download anything it tells you how long the clip is and how big the file will be. Audio files
are large: a quarter of an hour is around 80 MB, and an hour is around 330 MB. Check the number before you
download over a slow connection.

Two things worth knowing:

- If your chosen range covers a stretch where nothing was recorded, that stretch comes out as **silence**
  rather than being skipped. This keeps the file the same length as the time range, so times in the file
  still match times on the timeline.
- The last few seconds before the present cannot be exported yet, for the same reason they cannot be
  rewound to yet.

## Checking that it is still recording

![The recorder panel, showing the recording badge, elapsed time and the microphone level meter](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/recorder.png)

The red **Recording** badge and the timer next to it are the answer to "is this thing on?".

Below them is the level meter, which shows how loud the microphone is right now. Aim for the bar to sit in
the middle most of the time. If it is barely moving the microphone is too quiet or muted; if it is pinned
at the right hand end the sound is too loud and will be distorted. Both are fixed by the input gain in
[Settings](#settings), or by moving the microphone.

**Stop** halts the recording. Nothing is captured while it is stopped, and that period will be a blank gap
on the timeline for ever. Press it only when you mean to.

It stops the *recording*, not the service. The page keeps working and everything already recorded stays
playable. Shutting the service down itself is a separate thing, covered in the
[installation guide](SETUP.md#stopping-it).

The remaining rows are for support purposes. **Dropped frames** should stay at zero; a number climbing
there means the computer cannot keep up with the audio.

## Disk space

![The storage panel, showing disk used, how far back the history goes and the recent sessions](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/storage.png)

- **On disk** is how much space the recordings take up right now.
- **History** is how far back the recordings actually reach.
- **Retention** is how far back it is *allowed* to reach before old audio is deleted automatically.
- **Oldest** is the earliest moment you can still listen to.

Underneath is a list of recent recording sessions, one per time the recorder was started.

The service deletes its own old recordings, so this figure levels off rather than growing for ever. Where
it levels off is set by the retention window, described next.

## Settings

![The settings page](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/settings.png)

Settings work differently from the rest of the app: **nothing you change takes effect until you press Save
changes.** A bar appears at the bottom of the page with three buttons.

- **Save changes** applies everything you have edited.
- **Discard** throws your edits away and puts the previous values back.
- **Restore defaults** fills the page with the original settings. It is still only a proposal until you save
  it, so you can look at what the defaults are and then discard them.

### How long to keep recordings, and at what quality

![The history and storage settings, with the bit rate, the retention window and the storage estimate](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/settings-storage.png)

This one card decides how much disk the service will use, which for most people is the only setting that
matters.

**Delete recordings older than** sets how far back the timeline reaches. Anything older is deleted within a
minute of passing the limit. Type a number and choose hours or days, or use one of the preset buttons.

**Keep everything forever** never deletes anything. Only choose this if you are watching the disk yourself,
because when the disk fills up the recording stops.

**Storage needed** does the arithmetic for you. It shows how fast the disk is filling, how much is used
today, and how much you would need if the recorder ran non stop for the whole retention window. That last
figure is a worst case, so real usage is usually lower.

**Recording bit rate** trades sound quality for disk space:

![The bit rate menu, from full quality down to telephone quality, each with its disk cost per hour](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/settings-bitrate.png)

Each option tells you what it costs per hour. **Match the device** keeps whatever quality the microphone
provides. If you are recording speech and want more history out of the same disk, **Voice, 256 kbps** uses
about a third of the space and speech stays perfectly clear. Lower rates lose the high frequencies first,
which speech barely uses but music does.

A change of bit rate applies to the **next** recording session. Recordings already on disk keep the quality
they were made at, and still play normally.

### Where recordings are stored

![The recording location settings, with the directory tested and confirmed writable](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/settings-location.png)

By default recordings sit next to the service's own data. You can send them somewhere else, such as a
larger or external drive.

Type the folder path **as it exists on the host computer**, not on the computer you are sitting at. Press
**Test** before saving and it will tell you whether that folder exists and can be written to, which is much
better than finding out later that nothing was recorded.

A change here applies to the next recording session. Recordings already written stay where they are and go
on playing normally, so you can move the location without losing history.

### Microphone level, block length and start up

![The audio settings: input gain, segment length and record on start up](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/settings-audio.png)

**Input gain** makes a quiet microphone louder. Watch the level meter on the control room page while you
adjust it. Above 1.00x you risk distorting an already loud microphone.

**Segment length** is how big the blocks are that audio is written to disk in. It has one visible effect:
recent audio only becomes rewindable once its block is complete, so a shorter block makes the last few
seconds available sooner. Ten seconds suits almost everyone.

**Record on start up** makes the service begin recording the moment the host computer starts it, without
waiting for anyone to open the page. Leave this on if the point is to have a recording running whether or
not anyone is watching.

## On a phone or a tablet

![The full interface on a narrow phone screen, stacked into a single column](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/phone.png)

The same web address works on a phone or tablet on the same network. The layout stacks into one column and
everything still works, including the timeline, which responds to touch: tap to move, drag to pan, pinch to
zoom.

## Dark mode

![The control room in dark mode](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/dark-mode.png)

The moon and sun icon in the top right corner switches between light and dark. Your choice is remembered in
that browser, and it affects nobody else.

## Questions and problems

**I pressed play and hear nothing.**
Check your device volume and the volume slider in the app first. Then look at the level meter in the
Recorder panel: if it is not moving, no sound is reaching the microphone, and the problem is the microphone
rather than your browser.

**It says the stream is disconnected.**
Your browser has lost contact with the host computer. It retries by itself, so wait a few seconds. If it
does not come back, check that the host computer is still switched on and still on the network. Reloading
the page is harmless and often enough.

**No microphones are listed.**
On macOS the very first run asks for permission to use the microphone. If that was refused, the service
sees no microphones at all. Grant it under System Settings, Privacy and Security, Microphone, then restart
the service. Pressing the refresh button next to "Input source" looks for devices again.

**The last few seconds cannot be rewound to.**
That is expected, and it is explained under
[Going back to an earlier moment](#going-back-to-an-earlier-moment).

**Old recordings have vanished.**
They passed the retention window and were deleted automatically. Increase "Delete recordings older than"
in Settings if you need to keep more, and check the storage estimate before you do, because a longer window
needs proportionally more disk.

**The disk is filling up.**
Either shorten the retention window or choose a lower bit rate. Both are in Settings, and the page shows
you what each choice costs per hour.

**Somebody else changed a setting.**
There is no login, so everyone who can open the page has the same powers. Settings changes affect the
recording for everybody.

**Can I listen from outside the building?**
Not as it stands. The service is designed for a local network and has no authentication, so it should not
be exposed to the internet directly. Ask whoever administers it about a VPN.

## Reporting a problem

There is a link at the bottom of every page:

![The footer strip, with the copyright, the licence, a link to star the repository, a report an issue link and the version number](https://raw.githubusercontent.com/shibbirweb/on-air-record/master/docs/images/user-guide/footer.png)

If something does not work, or the guide is wrong, open an issue:

**https://github.com/shibbirweb/on-air-record/issues**

Please include:

- What you did, what you expected, and what happened instead.
- Your operating system, and the version number from that same strip, `v0.1.0` above.
- Anything the program printed in its window or log at the time.

## Credits

On Air Record is built and maintained by **MD. Shibbir Ahmed**
([portfolio](https://shibbirweb.github.io), [GitHub](https://github.com/shibbirweb)).

Released under the [MIT licence](https://github.com/shibbirweb/on-air-record/blob/master/LICENSE). Copyright (c) 2026 MD. Shibbir Ahmed.
