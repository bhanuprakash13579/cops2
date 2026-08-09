# Windows test plan — the backup and upgrade paths

Everything here has been verified on Linux against a copy of the real Chennai
database (827,140 rows). None of it has run on Windows. This lists only the
things that behave **differently on Windows**, ordered by what failure would
cost. Each has an expected result, so a run either passes or does not.

Two machines: **PC-A** running COPS2, **PC-B** holding a shared backup folder.

---

## 1. The upgrade — do this FIRST, on a copy

The highest-risk operation in the program: on success it securely wipes the
cops1 database. It is verified before that happens, but verification has only
ever run on Linux.

**Setup.** Copy a real `cops_br_database.db` into
`%APPDATA%\gov.in.customs.cops\` on PC-A. **Keep a copy elsewhere** — not as a
safety net for the program, but so a failed test is repeatable.

| Step | Expected |
|---|---|
| Start COPS2 with only `cops_br_database.db` present | Log: `migration verified: N tables, M rows, all counts match` |
| Check the folder afterwards | `cops.db` exists; `cops_br_database.db` is **gone** |
| Open the app | Every OS/BR/DR case present; counts match the old app |
| Check print templates in admin | Same templates as before, not defaults |

**If the wipe fails but the migration succeeded** — likely on Windows, where an
open file cannot be deleted — the log says `Could not delete …`. That is not
data loss, but tell me: it means the cops1 file is being held open somewhere.

**Do not proceed past this step if row counts do not match.**

---

## 2. `cops.db` and `cops_br_database.db` side by side

The bug found on the Linux machine: a leftover `cops.db` hid a full cops1
database. Worth confirming the fix works with Windows file locking.

| Step | Expected |
|---|---|
| Put a small/empty `cops.db` beside a full `cops_br_database.db`; start | Log: `REFUSING TO USE THE SMALLER DATABASE` |
| Check the folder | A `cops.db.ignored-<timestamp>` file exists — **not deleted** |
| Open the app | The full data is present |

---

## 3. Backup folders on another machine

This is where Windows differs most, and where a silent failure matters most.

Set the backup folder in **Admin → Backup** to a UNC path: `\\PC-B\COPS-Backup`.

| Step | Expected |
|---|---|
| Save the folder | Shown with a green dot and **"another machine"** |
| A local path e.g. `C:\Temp\bk` | Green dot, **"this machine"**, plus the warning that backups on this PC die with this PC |
| Mapped drive `Z:\bk` (Z: mapped to PC-B) | **"another machine"** — if it says "this machine", `is_remote` is wrong on Windows; tell me |
| Wait ~2 minutes after launch | A `cops_auto_<date>_<time>.cops` appears on PC-B, about 45 MB |
| Leave it running an hour | At most **2** `.cops` files in the folder, never 3 |

### The disconnected-share case — the one that used to hang the app

| Step | Expected |
|---|---|
| Switch PC-B off (or unplug the network); start COPS2 on PC-A | App window appears in the normal time. **If it hangs for a minute, the probe is not working on Windows** |
| Admin → Backup | The folder shows a red dot and a reason |
| The app itself | Fully usable — booking cases must not be affected |
| Switch PC-B back on; wait one interval | A backup appears on PC-B **without anyone booking a new case** |

That last row is the catch-up rule. It is the one most likely to be quietly
broken, because nothing looks wrong when it fails.

---

## 4. Saving a backup by hand

| Step | Expected |
|---|---|
| Admin → Backup → **Save Backup** | Windows save dialog, default name `cops_backup_<date>.cops` |
| Save to the Desktop | Message naming the record count; file about 45 MB |
| **Save again to the SAME file, confirm overwrite** | **Succeeds.** This is the `fs::rename` difference — on Windows a rename onto an existing file fails, and this is the fix for it. If you get "Access is denied", tell me |
| Save directly to `\\PC-B\COPS-Backup\test.cops` | Succeeds |
| While saving, watch the folder you saved into | **No large `.tmp` file appears there.** The plaintext intermediate must stay in the temp directory — if a ~180 MB file appears on your USB stick or share, that is unencrypted case data and I need to know |
| Open the `.cops` in 7-Zip, password = the binding secret | Opens; contains `cops_data.db` and `manifest.json` |

---

## 5. Restore — test on a machine you can afford to break

| Step | Expected |
|---|---|
| Restore a backup taken minutes ago | Succeeds; reports records restored; names a `cops_before_restore_…cops` file **beside cops.db**, not in Temp |
| Restore a backup from much earlier, holding fewer cases | **Refused**, naming the tables and the counts |
| Confirm the loss explicitly and retry | Succeeds |
| Restore a file that is not a backup (rename any zip to `.cops`) | Refused with "Nothing has been changed"; app still works, data intact |
| After a real restore | Restart, then confirm cases, templates and duty rates are all as expected |

---

## 6. Left running

| Step | Expected |
|---|---|
| Leave COPS2 running overnight with backups configured | Admin → Backup shows a recent success, no red warning |
| Check `%TEMP%` | No accumulation of `cops_bk_*`, `cops_archive_*`, `cops_plain_*` files |
| Break the backup folder (rename the share) and leave it a day | Panel says **"No backup has succeeded in the last 24 hours"** |

---

## 7. Things worth trying because they break software

- A backup folder path longer than 260 characters
- A folder on a drive with less free space than the archive
- Antivirus on real-time scanning — it can lock a file between write and rename
- Closing the app **while a backup is running** (during the first 2 minutes),
  then reopening: no `.partial` file should be left behind after an hour, and
  the next backup must work normally
- Two officers on PC-A and PC-B both backing up to the **same** shared folder —
  worth knowing what happens, as retention is per-folder and they will prune
  each other's copies. Tell me if you intend this; it needs a change

---

## What to send me

For anything that fails: the step, what happened, and the log. Logs are the
app's console output — if that is hard to capture on Windows, say so and I will
add file logging first.

Most useful of all: the **exact wording** of any message that was confusing or
unhelpful. Those are meant to be read by an officer under pressure, and I have
only ever read them myself.
