# Installing and using Axe-Grid

This guide is for people who want to use the app. You do not need to know anything about
programming. If you want to build it yourself, see [CONTRIBUTING.md](../CONTRIBUTING.md).

- [What you need](#what-you-need)
- [Windows](#windows)
- [macOS](#macos)
- [Linux](#linux)
- [Connecting your Axe-Fx II](#connecting-your-axe-fx-ii)
- [First steps](#first-steps)
- [Things to know](#things-to-know)
- [Troubleshooting](#troubleshooting)
- [Updating and removing](#updating-and-removing)

## What you need

- An Axe-Fx II, II XL or II XL+. (Other Fractal Audio products use a different protocol and will
  not work.) So far Axe-Grid has been tested on an Axe-Fx II XL with firmware 32.00.
- A computer with Windows 10 or 11, macOS 11 or newer, or a current Linux desktop.
- A way for the computer to reach the unit over MIDI: its USB connection if your system shows the
  unit as a MIDI device, or any MIDI interface with two 5-pin cables.

You can also try Axe-Grid with no unit at all: press **Simulator** after starting it.

Axe-Grid is early alpha software. Before you use it on presets you care about, make a backup of
your unit with the tools you already trust.

## Windows

1. Open the [latest release](https://github.com/RamonWide/axe-grid/releases/latest) and download
   `axe-grid-<version>-windows-x64.zip`.
2. Right-click the downloaded file and choose **Extract All…**. Any folder will do, such as
   Downloads.
3. Open the extracted folder and double-click **axegrid-setup.exe**. A small window lists what it
   does and asks you to press Enter when it is finished.
4. The first time, Windows shows a blue box saying **"Windows protected your PC"**. This appears
   because the app is not yet signed with a paid certificate. Click **More info**, then
   **Run anyway**. You may see it once more when you first start Axe-Grid itself.
5. Start Axe-Grid from the new **Axe-Grid** shortcut on your desktop, or from the Start menu.

The installer copies the app into your own user account (`AppData\Local\Programs\Axe-Grid`), so
it does not ask for an administrator password. It adds the two shortcuts and an entry under
**Settings › Apps › Installed apps**, and nothing else: no drivers, no background services. After
it has run you can delete the downloaded folder.

Prefer not to install? Double-click **axegrid.exe** in the extracted folder instead; the app is
that one file and runs from anywhere.

## macOS

1. Open the [latest release](https://github.com/RamonWide/axe-grid/releases/latest) and download
   `axe-grid-<version>-macos-universal.zip`. It works on both Apple silicon and Intel Macs.
2. Double-click the downloaded file to unzip it, then drag **Axe-Grid** into your **Applications**
   folder.
3. The first time, macOS refuses to open it because the app is not yet notarized by Apple:
   - Double-click **Axe-Grid**. A message says it cannot be opened. Click **Done**.
   - Open **System Settings › Privacy & Security**, scroll down to the message about Axe-Grid, and
     click **Open Anyway**. Confirm with your password.
   - On macOS 14 and older you can instead right-click the app, choose **Open**, then **Open**
     again.

You only do this once.

## Linux

1. Open the [latest release](https://github.com/RamonWide/axe-grid/releases/latest) and download
   `axe-grid-<version>-linux-x86_64.tar.gz` (most PCs) or `-linux-aarch64.tar.gz` (ARM computers).
2. Unpack it and start the app:

   ```sh
   tar -xzf axe-grid-*-linux-x86_64.tar.gz
   cd axe-grid-*-linux-x86_64
   ./axegrid
   ```

3. If it reports a missing library, install the sound library it needs:

   | Distribution | Command |
   |---|---|
   | Debian, Ubuntu, Mint | `sudo apt install libasound2` (named `libasound2t64` on Ubuntu 24.04 and newer) |
   | Fedora | `sudo dnf install alsa-lib` |
   | Arch | `sudo pacman -S alsa-lib` |

To add Axe-Grid to your application menu, run `./install-desktop-entry.sh` in the unpacked folder.
It copies a launcher and the icon into `~/.local/share`; nothing is installed system-wide.

The builds need glibc 2.35 or newer (Ubuntu 22.04, Debian 12, Fedora 36 and later).

## Connecting your Axe-Fx II

Axe-Grid talks to the unit over a MIDI port. There are two ways to get one.

**USB.** Connect the unit with a USB cable and follow Fractal Audio's own instructions for your
operating system, including their driver where one is required. If the unit then appears in your
system as a MIDI device, Axe-Grid can use it. On one Windows 11 computer we tested, Windows
refused to load the vendor's USB driver, so no MIDI port appeared; a MIDI interface worked.
If that happens to you, the open-source [AxeTwoDriver](https://github.com/RamonWide/AxeTwoDriver)
makes the unit's USB MIDI port appear again (as "AXE-FX II Audio Intfc"); it is experimental and
has been tried on one computer.

**A MIDI interface.** Any interface with 5-pin sockets works. Use two cables:

- interface **MIDI OUT** → Axe-Fx **MIDI IN**
- Axe-Fx **MIDI OUT** → interface **MIDI IN**

Both directions are needed: the editor asks, and the unit answers.

Then:

1. Close any other program that talks to the unit (other editors, librarians, a DAW with the port
   open). On Windows only one program can use a MIDI port at a time.
2. Switch the unit on and start Axe-Grid. It looks for the unit by itself: first on the unit's own
   USB port, then on every other MIDI port. When it finds it, the current preset appears on the
   grid within a second or two and the status line at the bottom says which port it used.
3. If the unit was off or busy, press **Find my Axe-Fx** to look again.
4. To choose the port yourself, pick it under **MIDI IN** and under **OUT** and press
   **Connect**. If the lists are empty, switch the unit on and press **Rescan**.

## First steps

- **Presets:** the arrows beside the preset number step up and down. Type a number into the box
  and press Enter to jump.
- **Scenes:** the buttons 1 to 8 under the preset name.
- **Edit a block:** click it on the grid. Its pages appear below; the tabs switch pages.
- **Change a value:** drag a knob up or down, or turn the mouse wheel over it. Hold **Shift** for
  fine steps. To type a value, double-click the number under a knob or a fader, type the new
  number and press Enter; Escape leaves it as it was.
- **Bypass and X/Y:** the buttons beside the block's name.
- **Add a block:** right-click an empty cell and choose from the list. Right-click a block to
  remove it.
- **Move a block:** drag it to an empty cell.
- **Cable two blocks:** hover the first block and click the **O** on its right, then click the
  **I** on the left of a block in the next column. To run a cable all the way to the start or the
  end of the grid, click a handle and then the **INPUT** or **OUTPUT** bar.
- **Input gate and output mixer:** click the **INPUT** or **OUTPUT** bar.
- **Tuner, tap tempo, BPM:** in the top bar.
- **All your presets:** the **Presets** button opens a list of every slot with its name. Type in
  **Find** to search. Double-click a preset to load it. With a preset picked you can **Export…**
  it to a file, copy it to another slot, or store what you are hearing into that slot
  (**Save current here**). **Import file…** loads a preset file for you to try: it is not stored
  until you save it. **Back up all…** writes every preset into a folder you choose, one file each;
  it only reads from the unit.
- **User cabs** (XL and XL+): the **Cabs** button lists the 1024 user cab slots. Pick one to
  **Export…** it to a file, to load a cab file into that slot (**Import file here…**), or to hear
  it in the current preset (**Use in cab block**). Importing a cab is new and has not yet been
  confirmed on a unit, so export the slot first if it holds something you care about.
- **Curves:** equalisers, filters, the compressor, the gates and some other blocks draw a curve
  beside their controls. It is worked out from the settings as a guide; it is not a measurement
  of the sound.
- **Save:** the **Save** button stores the preset in its slot on the unit, after asking you to
  confirm. Until you save, your changes live only in the unit's edit buffer, exactly as when you
  turn the knobs on its front panel; selecting another preset discards them.

## Things to know

- **Lists of types.** Names in drop-down lists (amp types, cab names, reverb types and so on) are
  read from a file the original editor keeps on your computer. If Axe-Edit has been run on the
  same computer for your model, Axe-Grid finds that file by itself and the lists are complete.
  Without it, lists show the current value as the unit reports it, and you step through them with
  the mouse wheel. Reading the names from the unit itself is on the [roadmap](../ROADMAP.md).
- **No undo yet.** To get back to the saved preset, select it again.
- **Anything that overwrites asks first.** Saving a preset, copying one over another and importing
  a cab each show which slot will be replaced before anything is sent.
- **Not everything is there.** Modifiers, global settings and more are still missing; see the
  [roadmap](../ROADMAP.md).
- **Firmware.** Axe-Grid never touches the unit's firmware or its global settings.

## Troubleshooting

| What you see | What to try |
|---|---|
| "No Axe-Fx found" when the app starts | Switch the unit on and press **Find my Axe-Fx**. Close other programs that use the unit. If it still is not found, pick the ports yourself. |
| The port lists are empty | Switch the unit on, check the cable, press **Rescan**. With USB, check that your system shows the unit as a MIDI device at all. |
| **Connect** gives "no answer" | Another program may hold the port: close it. With a MIDI interface, check that both cables are plugged in and not swapped. Make sure you picked the unit's port for both IN and OUT. |
| It connects but the window stutters | The unit is slow to answer or a cable is loose. Disconnect, check the cables, connect again. |
| Lists show only one value | See "Lists of types" above. |
| Windows says the app is not safe | See step 4 under [Windows](#windows). |
| macOS says the app is damaged or cannot be opened | See step 3 under [macOS](#macos). If it still refuses, open Terminal and run `xattr -dr com.apple.quarantine /Applications/Axe-Grid.app`. |
| Linux: "error while loading shared libraries" | Install the library named in the message; see the table under [Linux](#linux). |
| The unit stops responding | Close Axe-Grid and switch the unit off and on. Nothing is stored unless you pressed Save. Then please [report it](https://github.com/RamonWide/axe-grid/issues/new/choose) with what you were doing. |

Still stuck? [Open an issue](https://github.com/RamonWide/axe-grid/issues/new/choose) and tell us
your Axe-Fx II model, its firmware version, your operating system and how the unit is connected.

## Updating and removing

To update, download the newer release and install it the same way; on Windows, close Axe-Grid
first and run the new **axegrid-setup.exe**, which replaces the old files.

To remove Axe-Grid on Windows, open **Settings › Apps › Installed apps**, find **Axe-Grid** and
choose **Uninstall**; that removes the app and both shortcuts. If you never ran the installer,
delete the folder. On Linux delete the folder, and on macOS drag the app to the Bin. Axe-Grid
keeps no settings elsewhere on your computer and installs no drivers or background services.

## Checking your download (optional)

Each release has a `SHA256SUMS.txt` file listing a checksum for every download. To check a file:

```sh
# Windows (PowerShell)
Get-FileHash .\axe-grid-<version>-windows-x64.zip -Algorithm SHA256
# macOS
shasum -a 256 axe-grid-<version>-macos-universal.zip
# Linux
sha256sum -c SHA256SUMS.txt --ignore-missing
```

The printed value should match the line for that file in `SHA256SUMS.txt`.
