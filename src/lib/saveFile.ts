/**
 * Putting a file the officer asked for onto a disk they choose.
 *
 * The app runs in a desktop window, not a browser tab. The usual web trick —
 * make a blob, point a hidden link at it, click the link — does nothing there:
 * no download bar, no file, no error. That is why the monthly backup reminder
 * appeared to do nothing at all when its button was pressed.
 *
 * So a save goes through the operating system's own Save dialog, and the bytes
 * are written where the officer put them. The link trick stays as a fallback for
 * when the app is opened in an ordinary browser, where it does work.
 */

/** Where the file went, or null if the officer closed the dialog. */
export type SaveResult = { path: string | null; cancelled: boolean };

export async function saveBlob(
  data: Blob | ArrayBuffer | Uint8Array,
  defaultName: string,
  opts: { title?: string; extensions?: string[] } = {},
): Promise<SaveResult> {
  const bytes =
    data instanceof Blob ? new Uint8Array(await data.arrayBuffer())
    : data instanceof Uint8Array ? data
    : new Uint8Array(data);

  try {
    const { save } = await import('@tauri-apps/plugin-dialog');
    const { writeFile } = await import('@tauri-apps/plugin-fs');
    const path = await save({
      title: opts.title ?? 'Save',
      defaultPath: defaultName,
      filters: opts.extensions?.length
        ? [{ name: opts.extensions[0].toUpperCase(), extensions: opts.extensions }]
        : undefined,
    });
    if (!path) return { path: null, cancelled: true };
    await writeFile(path, bytes);
    return { path, cancelled: false };
  } catch (err) {
    // Not running inside the desktop window — fall back to the browser's own
    // download. Any other failure is real and belongs to the caller.
    const msg = String(err);
    const outsideTheApp = msg.includes('plugin-dialog')
      || msg.includes('plugin-fs')
      || msg.includes('__TAURI_IPC__')
      || msg.includes('not a function');
    if (!outsideTheApp) throw err;

    const url = URL.createObjectURL(new Blob([bytes as BlobPart]));
    const a = document.createElement('a');
    a.href = url;
    a.download = defaultName;
    document.body.appendChild(a);
    a.click();
    a.remove();
    // Not revoked straight away: the browser is still reading from that URL when
    // the click returns, and pulling it away cancels the download of a large
    // file — the officer gets a download that starts and then simply stops.
    setTimeout(() => URL.revokeObjectURL(url), 60_000);
    return { path: defaultName, cancelled: false };
  }
}
