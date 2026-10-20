diskdash {version}  (portable, Windows 10/11 64-bit)
================================================

diskdash is a local dashboard for the drives in this PC. It shows what is on each
drive, where the space goes, and what you could remove or move. It is strictly
read-only: it finds and explains, and you carry out every step yourself.

Start
-----
1. Unzip this folder anywhere (for example your Documents folder).
2. Double-click "Start diskdash.cmd". Your browser opens on http://127.0.0.1:8765.
3. Close the black window to stop diskdash.

There is nothing to install and nothing runs in the background. diskdash only
listens on 127.0.0.1 (this PC), makes no internet connections, and shows no
Windows notifications.

If Windows SmartScreen warns about an unrecognised app, that is because the
files are not code-signed. Choose "More info" and then "Run anyway" if you trust
whoever sent you this zip. You can compare the zip's SHA-256 with the .sha256
file that came with it.

Faster scans (optional)
-----------------------
In the Scan dialog, "Enable elevated features" starts diskdash-helper.exe after a
Windows administrator prompt. The helper only reads the drive's file table and
change journal, which makes a whole-drive scan take seconds instead of minutes.
It cannot write, delete or run anything. If you decline the prompt, diskdash
works normally with the slower folder walk.

Where it keeps its data
-----------------------
%LOCALAPPDATA%\diskdash  (scan results, settings, logs). Delete that folder to
start fresh. diskdash never changes your files; reports are saved only where you
choose.

Options
-------
Run  diskdash.exe --port 9000  from a command prompt to use another port.

Keep diskdash.exe and diskdash-helper.exe in the same folder.
