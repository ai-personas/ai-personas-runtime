(() => {
    "use strict";
    const sheet = document.getElementById("field-report");
    const observer = document.getElementById("field-observer");
    const observations = document.getElementById("field-observations");
    const originals = document.getElementById("field-originals");
    const save = document.getElementById("field-save");
    const status = document.getElementById("field-status");

    function updatePrint() {
        document.getElementById("printed-observer").textContent = observer.value;
        document.getElementById("printed-observations").textContent = observations.value;
        document.getElementById("printed-evidence").textContent =
            Array.from(originals.files, file => file.name).join(", ");
    }
    sheet.addEventListener("input", updatePrint);
    window.addEventListener("beforeprint", updatePrint);
    save.disabled = false;
    save.addEventListener("click", async () => {
        if (!observer.reportValidity() || !observations.reportValidity()) return;
        if (!observer.value.trim() || /[\u0000-\u001f\u007f-\u009f]/u.test(observer.value) || !observations.value.trim()) {
            status.textContent = "Enter your name or role and what you observed.";
            return;
        }
        const report = {
            format: sheet.dataset.format,
            request: sheet.dataset.request,
            request_sha256: sheet.dataset.digest,
            observer: observer.value,
            observations: observations.value,
            evidence: []
        };
        save.disabled = true;
        try {
            for (const file of Array.from(originals.files)) {
                if (/[/\\:\u0000-\u001f\u007f-\u009f]/u.test(file.name) || !file.name.trim() || [".", ".."].includes(file.name)) {
                    throw new Error("Give the evidence file a portable name and select it again.");
                }
                if (!window.crypto || !crypto.subtle) {
                    throw new Error("Open the saved Request.html in a browser that supports local file checksums.");
                }
                status.textContent = "Checking " + file.name + "…";
                const hash = new Uint8Array(await crypto.subtle.digest("SHA-256", await file.arrayBuffer()));
                const sha256 = Array.from(hash, byte => byte.toString(16).padStart(2, "0")).join("");
                if (report.evidence.some(entry => entry.name === file.name && entry.sha256 === sha256)) continue;
                report.evidence.push({
                    name: file.name,
                    bytes: file.size,
                    sha256
                });
            }
            const blob = new Blob([JSON.stringify(report, null, 2) + "\n"], {type: "application/json"});
            const link = document.createElement("a");
            const url = URL.createObjectURL(blob);
            link.href = url;
            link.download = "Field-report-" + report.request.replace(/[^a-zA-Z0-9-]+/g, "-") + ".json";
            document.body.append(link);
            link.click();
            link.remove();
            window.setTimeout(() => URL.revokeObjectURL(url), 30000);
            status.textContent = report.evidence.length
                ? "Return the downloaded report and the selected original files together."
                : "Return the downloaded report to the person handling this request in Studio.";
        } catch (error) {
            status.textContent = error.message || "The report could not be saved.";
        } finally {
            save.disabled = false;
        }
    });
})();
