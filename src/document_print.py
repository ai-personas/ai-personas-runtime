"""WeasyPrint worker for an in-memory document and its declared project files."""

import base64
import binascii
import contextlib
import json
import logging
import mimetypes
import os
import sys
from urllib.parse import quote, unquote, unquote_to_bytes, urlsplit

# Keep native dependency guidance away from the result protocol.
try:
    with contextlib.redirect_stdout(sys.stderr):
        import weasyprint
        from PIL import Image
        from weasyprint import HTML
        from weasyprint.text.fonts import FontConfiguration
        from weasyprint.urls import URLFetcher, URLFetcherResponse
    if int(weasyprint.__version__.split(".")[0]) < 70:
        sys.exit(69)
except (ImportError, OSError):
    sys.exit(69)

# An ordinary hierarchical URL lets the layout engine correctly resolve URLs
# relative to nested stylesheets, SVGs and the document. Nothing opens this host.
PROJECT = "https://print.invalid/"


class PrintProblem(BaseException):
    # WeasyPrint normally recovers from resource errors by omitting the image
    # or stylesheet. A requested resource must be present for a successful print.
    pass


def protect_process():
    scratch = os.path.realpath(os.getcwd())
    runtime = tuple(os.path.realpath(p) for p in sys.path if os.path.isabs(p))

    def within(path, root):
        return path == root or path.startswith(root + os.sep)

    def audit(event, args):
        if event in ("subprocess.Popen", "os.system", "os.exec", "os.posix_spawn",
                     "os.spawn", "os.fork", "os.forkpty", "socket.__new__"):
            raise PrintProblem("Static printing cannot start external programs or connections.")
        if event == "open" and isinstance(args[0], (str, bytes)):
            path = os.path.realpath(os.fsdecode(args[0]))
            writing = args[2] & (os.O_CREAT | os.O_WRONLY | os.O_RDWR | os.O_TRUNC)
            if within(path, scratch) or (not writing and any(within(path, p) for p in runtime)):
                return
            raise PrintProblem("The document requested a file outside its supplied resources.")

    sys.addaudithook(audit)


def decode_data(url):
    media, separator, encoded = url[5:].partition("#")[0].partition(",")
    if not separator or "\n" in media or "\r" in media:
        raise PrintProblem("The document contains a malformed data URL.")
    payload = unquote_to_bytes(encoded)
    if media.lower().endswith(";base64"):
        media = media[:-7]
        try:
            payload = base64.b64decode(b"".join(payload.split()), validate=True)
        except (ValueError, binascii.Error):
            raise PrintProblem("The document contains invalid base64 in a data URL.") from None
    if not media or media.startswith(";"):
        media = "text/plain" + media
    return payload, media


class ProjectFiles(URLFetcher):
    def __init__(self, resources):
        super().__init__(allowed_protocols=(), fail_on_errors=True)
        self.resources = resources
        self.problem = None

    def fetch(self, url, headers=None):
        try:
            content, media = self.resolve(url)
            return URLFetcherResponse(url, content, {"Content-Type": media})
        except PrintProblem as error:
            # Some embedded image formats recover even from fatal fetch errors.
            self.problem = error
            raise

    def resolve(self, url):
        if url[:5].lower() == "data:":
            return decode_data(url)
        try:
            parsed = urlsplit(url)
            path = unquote(parsed.path.removeprefix("/"), errors="strict")
        except ValueError:
            raise PrintProblem("The document contains an invalid resource URL.") from None
        if parsed.scheme != "https" or parsed.netloc != "print.invalid":
            raise PrintProblem(
                "An external resource was requested. Supply it at a relative project "
                "path or embed it as a data URL.")
        if path not in self.resources:
            # Never include query strings, remote URLs or data in a receipt.
            label = "".join(c if c.isprintable() else "?" for c in path)
            raise PrintProblem(f"Missing print resource at project path {label!r}.")
        return self.resources[path], mimetypes.guess_type(path)[0] or "application/octet-stream"


class Fonts(FontConfiguration):
    def add_font_face(self, descriptors, url_fetcher):
        # WeasyPrint converts CSS local() values into file URLs. System font
        # families still resolve through Pango. Explicit font files must come
        # through the project, including a URL fallback to local().
        sources = [src for src in descriptors["src"] if src[0] != "local"]
        if sources:
            super().add_font_face({**descriptors, "src": sources}, url_fetcher)


class LayoutErrors(logging.Handler):
    def emit(self, record):
        message = str(record.msg)
        if record.levelno >= logging.ERROR:
            if isinstance((cause := sys.exc_info()[1]), PrintProblem):
                raise cause
            if "image" in message.lower() or "svg" in message.lower():
                raise PrintProblem("WeasyPrint could not decode or draw a supplied image.")
            if "stylesheet" in message.lower():
                raise PrintProblem("WeasyPrint could not load an attached stylesheet as CSS.")
            raise PrintProblem("WeasyPrint could not render part of the supplied document.")
        if message.startswith("Font-face") and "cannot be loaded" in message:
            raise PrintProblem("WeasyPrint could not load a supplied font.")


def render():
    mimetypes.init(files=[])
    logging.getLogger("weasyprint").addHandler(LayoutErrors())
    # Pillow also registers formats that delegate to an external interpreter.
    # Keep its in-process raster decoders. WeasyPrint draws SVG itself.
    Image.init()
    passive = {"PNG", "JPEG", "MPO", "GIF", "WEBP", "BMP", "DIB", "ICO", "TIFF",
               "AVIF", "JPEG2000", "PPM", "PCX", "TGA", "XBM", "XPM", "QOI"}
    Image.ID[:] = [name for name in Image.ID if name in passive]
    pipe = sys.stdin.buffer
    source_path, length, files = json.loads(pipe.readline())
    source = pipe.read(length)
    fetcher = ProjectFiles({path: pipe.read(size) for path, size in files})
    protect_process()
    try:
        source = source.decode("utf-8-sig")
    except UnicodeDecodeError:
        pass  # Let the HTML5 parser honor BOMs and declared legacy encodings.
    document = HTML(
        string=source, media_type="print", url_fetcher=fetcher,
        base_url=PROJECT + quote(source_path or "", safe="/"),
    ).render(font_config=Fonts())
    # Printed forms keep their visible content. Only navigation within the PDF
    # is interactive; embedded files and external actions have no print content.
    document.metadata.attachments.clear()
    for page in document.pages:
        page.links[:] = [link for link in page.links if link[0] == "internal"]
        page.forms.clear()
    pdf = document.write_pdf()
    if fetcher.problem:
        raise fetcher.problem
    print(json.dumps({"pages": len(document.pages), "renderer": f"WeasyPrint {weasyprint.__version__}"}), flush=True)
    sys.stdout.buffer.write(pdf)


if __name__ == "__main__":
    try:
        render()
    except PrintProblem as problem:
        print(json.dumps({"error": str(problem)}))
        sys.exit(1)
    except (KeyboardInterrupt, SystemExit):
        raise
    except BaseException as problem:
        # Some decoder exceptions include the source bytes or a complete URL.
        print(json.dumps({"error": f"The document could not be printed ({type(problem).__name__})."}))
        sys.exit(1)
