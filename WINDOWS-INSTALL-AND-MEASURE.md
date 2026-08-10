# Windows: install, upgrade, and measure the space

Your plan, with what to check at each stage and the numbers to expect.

Both applications use the same Tauri identifier, so they share one folder:

```
C:\Users\<you>\AppData\Roaming\gov.in.customs.cops\
```

That is why COPS2 finds the old database in place and can upgrade it. Set this
once in PowerShell and every command below uses it:

```powershell
$APP = "$env:APPDATA\gov.in.customs.cops"
```

---

## The one command you will use most

Sizes of everything in the app folder, largest first:

```powershell
Get-ChildItem $APP -File |
  Sort-Object Length -Descending |
  Select-Object Name, @{n='MB';e={[math]::Round($_.Length/1MB,2)}}
```

And the folder total:

```powershell
"{0:N1} MB" -f ((Get-ChildItem $APP -Recurse -File |
  Measure-Object Length -Sum).Sum / 1MB)
```

Run it at each stage below and write the number down. The comparison between
stages is the answer you are after.

---

## Stage 1 — install OS_module_upgrade and restore the backups

1. Install and open it. **Measure now** — a fresh install should be a few MB.
2. Admin → Backup → **Restore** → `cops_backup_2026-08-06.zip`
   That endpoint takes exactly this file, and it is INSERT-ONLY: it adds
   missing records and never overwrites, so restoring twice is harmless.
3. Admin → **Restore Config** → `cops_config_backup_2026-08-06.json`
   Templates, masters, users and settings. Do this second — the case data does
   not depend on it, but the print templates do.

**Check the data is really there**, not just that the upload succeeded:

| where | expect |
|---|---|
| OS list | **29,100** cases |
| BR register | **334,546** |
| DR register | **14,138** |
| Admin → OS Template | **38** template rows |
| Users | **8** accounts |

Open one case and print it. If the heading and the order paragraphs look right,
the config restored properly — those come from the template rows.

**Measure again.** Expect roughly **250–260 MB**: SQLite stores the 827,140 rows
with indexes, which is why the 44 MB backup expands so much. That expansion is
normal and is the reason the backup is compressed at all.

---

## Stage 2 — install COPS2 over it

Install COPS2 **without deleting anything**. It looks in the same folder, finds
`cops_br_database.db`, and upgrades it.

**Watch for this in the log:**

```
migration verified: 90 tables, 827140 rows, all counts match
```

It verifies every table BEFORE it removes the old file. If the counts do not
match it refuses and leaves the original alone — so if you see anything else,
stop and tell me rather than continuing.

**Then check the folder:**

```powershell
Get-ChildItem $APP -File | Select-Object Name,
  @{n='MB';e={[math]::Round($_.Length/1MB,2)}}
```

| file | expect |
|---|---|
| `cops.db` | ~250–260 MB — the upgraded database |
| `cops_br_database.db` | **gone** — securely wiped after verification |
| `cops.db-wal` | small, a few MB at most |

If `cops_br_database.db` is still there, the wipe failed (Windows will not
delete a file something holds open). Not data loss, but tell me — it means
something still has a handle on it.

**Confirm the data survived**, in COPS2 this time: same five numbers as Stage 1.
Also open **Admin → Backup** and expand **"What is in the backup"** — it lists
the largest tables, which is the same evidence in the app itself.

---

## Stage 3 — measure what the backups actually cost

Set a backup folder in **Admin → Backup**, ideally on the second PC:

```
\\PC-B\COPS-Backup
```

Wait about two minutes after launch for the first automatic backup, then:

```powershell
$BK = "\\PC-B\COPS-Backup"
Get-ChildItem $BK -Filter *.cops |
  Select-Object Name, @{n='MB';e={[math]::Round($_.Length/1MB,2)}}, LastWriteTime
"{0:N1} MB total" -f ((Get-ChildItem $BK -Filter *.cops |
  Measure-Object Length -Sum).Sum / 1MB)
```

| what | expect |
|---|---|
| one archive | **~44 MB** |
| after several hours | **2 files, ~88 MB** — never three |
| filename | `cops_auto_<pcname>_<date>_<time>.cops` |

The two-file cap is the point: the folder cannot grow without limit however long
the machine runs.

**With both PCs backing up to the same folder**, expect **two files per machine**
— four in total, each named for the PC that wrote it. If PC-A's files vanish when
PC-B runs, tell me: that is the retention scoping failing, and it silently halves
your redundancy.

---

## The whole picture, in numbers

| stage | on disk |
|---|---|
| cops-web after restore | ~255 MB (the live database) |
| COPS2 after upgrade | ~255 MB (same data, re-encrypted) |
| one backup archive | 44 MB |
| a backup folder, steady state | 88 MB (two copies) |
| your two source files | 44.3 MB + 0.03 MB |

So a machine holding the app **and** a backup folder settles around **345 MB**,
and stops growing there apart from new cases.

The archive is 44 MB where the database is 255 MB because the backup drops the
indexes (they are rebuilt from the data), skips empty tables, and compresses
before encrypting. Encrypted bytes do not compress — that ordering is the whole
reason it is 44 MB and not 255 MB.

---

## If you want the exact figure for one component

Row counts per table, straight from the running app — Admin → Backup →
"What is in the backup". That reads the last archive's manifest, so it costs
nothing and tells you which table is actually taking the room. If any single one
starts to dominate later, that is the moment to reconsider excluding it, and the
setting is already there.
