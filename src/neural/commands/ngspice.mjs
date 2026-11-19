// This exact, digest-verified Emscripten release uses its browser runtime.
// Its module sees the project MEMFS, byte streams and clock, with no Node
// process, module loader or network transport in its realm.
import vm from "node:vm";
import os from "node:os";

const chunks = [];
for await (const chunk of process.stdin) chunks.push(chunk);
const input = JSON.parse(Buffer.concat(chunks).toString("utf8"));
const stdout = [], stderr = [];
const line = (stream, message) => {
    for (const byte of Buffer.from(String(message) + "\n")) stream.push(byte);
};
const byte = (stream, b) => { if (typeof b === "number") stream.push(b); };
const total = Math.floor(os.totalmem() / 1024);
const free = Math.floor(os.freemem() / 1024);
const report = {
    exit_code: null, error: null, files: Object.create(null), file_errors: Object.create(null),
    host: {runtime: "Node.js", version: process.version, interface: "Emscripten MEMFS",
        options: process.execArgv.slice(0, -1), environment_keys: Object.keys(process.env),
        virtual_meminfo: {total_kib: total, free_kib: free, source: "node:os totalmem and freemem"}},
};
try {
    const context = vm.createContext({TextEncoder, TextDecoder, atob, performance,
        crypto: {getRandomValues: a => globalThis.crypto.getRandomValues(a)},
        console: {log: m => line(stdout,m), warn: m => line(stderr,m), error: m => line(stderr,m)}});
    const module = new vm.SourceTextModule(Buffer.from(input.program,"base64").toString("utf8"), {
        context, identifier: "ngspice:module", initializeImportMeta(meta) {meta.url = "ngspice:module";}
    });
    await module.link(() => { throw Error("The pinned simulator has no external JavaScript imports"); });
    await module.evaluate();
    const data = Buffer.from(input.stdin, "base64");
    let cursor = 0;
    const m = await module.namespace.default({noInitialRun: true,
        stdin: () => cursor < data.length ? data[cursor++] : null,
        stdout: b => byte(stdout,b), stderr: b => byte(stderr,b),
        print: s => line(stdout,s), printErr: s => line(stderr,s)});
    const fs = m.FS;
    // ngspice 46 reads these two figures to size transient simulation output.
    fs.mkdirTree("/proc");
    fs.writeFile("/proc/meminfo", "MemTotal: " + total + " kB\nMemFree: " + free + " kB\n");
    fs.mkdirTree("/project");
    fs.chdir("/project");
    for (const [name, encoded] of Object.entries(input.files)) {
        const slash = name.lastIndexOf("/");
        if (slash >= 0) fs.mkdirTree(name.slice(0,slash));
        fs.writeFile(name, new Uint8Array(Buffer.from(encoded,"base64")));
    }
    const args = [input.name, ...input.arguments];
    const argv = m.stackAlloc(4 * (args.length + 1));
    for (const [index,arg] of args.entries()) {
        const size = m.lengthBytesUTF8(arg) + 1;
        const ptr = m.stackAlloc(size);
        m.stringToUTF8(arg,ptr,size);
        m.HEAPU32[(argv >>> 2) + index] = ptr;
    }
    m.HEAPU32[(argv >>> 2) + args.length] = 0;
    try { report.exit_code = m._main(args.length,argv); }
    catch (e) {
        if (e instanceof m.ExitStatus) report.exit_code = e.status;
        else report.error = String(e?.message ?? e);
    }
    for (const name of input.outputs) {
        try {
            const path = "/project/" + name;
            if (!fs.isFile(fs.lstat(path).mode)) throw Error("Output is not a regular file");
            report.files[name] = Buffer.from(fs.readFile(path)).toString("base64");
        } catch(e) { report.file_errors[name] = String(e?.message || (typeof e?.errno === "number" ? "Emscripten filesystem errno " + e.errno : e)); }
    }
} catch(e) { report.error = String(e?.message ?? e); }
report.stdout = Buffer.from(stdout).toString("base64");
report.stderr = Buffer.from(stderr).toString("base64");
process.stdout.write(JSON.stringify(report));
