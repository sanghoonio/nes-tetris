// The player's ROM, kept in this browser (IndexedDB) so it's asked for once. It never leaves the
// browser.

const DB = "nes-tetris";
const STORE = "rom";
const KEY = "tetris.nes";

function open(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

async function run<T>(mode: IDBTransactionMode, f: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> {
  const db = await open();
  return new Promise((resolve, reject) => {
    const req = f(db.transaction(STORE, mode).objectStore(STORE));
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

export async function loadRom(): Promise<Uint8Array | null> {
  const data = await run<ArrayBuffer | undefined>("readonly", (s) => s.get(KEY));
  return data ? new Uint8Array(data) : null;
}

export async function saveRom(rom: Uint8Array): Promise<void> {
  await run("readwrite", (s) => s.put(rom.slice().buffer, KEY));
}

export async function forgetRom(): Promise<void> {
  await run("readwrite", (s) => s.delete(KEY));
}
