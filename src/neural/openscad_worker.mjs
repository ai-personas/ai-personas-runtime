// The host verifies the compiler bytes before starting this process. Only the
// requested project files are written into the compiler's in-memory filesystem.
const chunks = [];
for await (const chunk of process.stdin) chunks.push(chunk);
const request = JSON.parse(Buffer.concat(chunks).toString("utf8"));
const messages = [];
const report = { node: process.version, messages };
try {
    const {createOpenSCAD} = await import("data:text/javascript;base64," + request.compiler);
    const record = line => messages.push(String(line));
    const instance = (await createOpenSCAD({print: record, printErr: record})).getInstance();
    if (request.version) {
        report.exit_code = instance.callMain(["--version"]);
    } else {
        const fs = instance.FS;
        const directories = new Set();
        for (const file of request.files) {
            let parent = "/project";
            for (const name of ["", ...file.path.split("/").slice(0, -1)]) {
                if (name) parent += "/" + name;
                if (!directories.has(parent)) {
                    fs.mkdir(parent);
                    directories.add(parent);
                }
            }
            fs.writeFile("/project/" + file.path, Buffer.from(file.bytes, "base64"));
        }
        const args = ["--export-format", "binstl", "-o", "/output.stl", "-d", "/dependencies.d"];
        for (const [name, value] of Object.entries(request.parameters)) {
            args.push("-D", name + "=" + JSON.stringify(value));
        }
        report.exit_code = instance.callMain([...args, "/project/" + request.entry]);
        // A failed compile may still return diagnostics or dependency information.
        if (fs.analyzePath("/dependencies.d").exists) report.dependencies = fs.readFile("/dependencies.d", {encoding:"utf8"});
        if (fs.analyzePath("/output.stl").exists) report.mesh = Buffer.from(fs.readFile("/output.stl")).toString("base64");
    }
} catch (error) {
    report.error = String(error?.message ?? error);
}
process.stdout.write(JSON.stringify(report));
