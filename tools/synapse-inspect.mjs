// Valuta un'espressione JavaScript nel processo principale di Synapse, per studiare Chroma Studio
// (opzioni dei layer, anteprima animata) senza usare il mouse.
//
// Synapse va avviato a mano con l'inspector di Node, che ascolta solo su 127.0.0.1:
//   "C:\Program Files\Razer\RazerAppEngine\app-<versione>\RazerAppEngine.exe" --url-params=apps=synapse,chroma-app --inspect=127.0.0.1:9230
// Finche' la porta e' aperta qualunque programma locale puo' comandare Synapse: chiuderlo a fine lavoro.
//
// Uso:  node tools/synapse-inspect.mjs "<corpo di una funzione async; return il risultato>"
//       es. node tools/synapse-inspect.mjs "return require('electron').BrowserWindow.getAllWindows().map(w => w.getTitle())"

const PORT = 9230;
const list = await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json();
const ws = new WebSocket(list[0].webSocketDebuggerUrl);
let id = 0;
const pending = new Map();
ws.onmessage = (e) => {
    const m = JSON.parse(e.data);
    pending.get(m.id)?.(m);
    pending.delete(m.id);
};
const call = (method, params = {}) =>
    new Promise((resolve) => {
        pending.set(++id, resolve);
        ws.send(JSON.stringify({ id, method, params }));
    });
await new Promise((resolve) => (ws.onopen = resolve));

const body = process.argv[2] ?? "return process.versions";
const expression = `(async () => { const require = process.mainModule.require.bind(process.mainModule); ${body} })()`;
const r = await call("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true });
const out = r.result?.exceptionDetails ?? r.result?.result?.value ?? r;
console.log(typeof out === "string" ? out : JSON.stringify(out, null, 1));
ws.close();
