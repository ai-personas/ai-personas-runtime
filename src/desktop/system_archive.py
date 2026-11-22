"""Read a system libarchive directory or stream one entry into a private file."""
import ctypes as c
import ctypes.util
import json
import os
import stat
import sys

lib = c.CDLL(ctypes.util.find_library("archive") or "libarchive.so")


def bind(name, result, *arguments):
    function = getattr(lib, name)
    function.restype = result
    function.argtypes = arguments
    return function


ptr = c.c_void_p
new = bind("archive_read_new", ptr)
filters = bind("archive_read_support_filter_all", c.c_int, ptr)
formats = bind("archive_read_support_format_all", c.c_int, ptr)
raw = bind("archive_read_support_format_raw", c.c_int, ptr)
open_file = bind("archive_read_open_filename", c.c_int, ptr, c.c_char_p, c.c_size_t)
archive_format = bind("archive_format", c.c_int, ptr)
filter_count = bind("archive_filter_count", c.c_int, ptr)
next_entry = bind("archive_read_next_header", c.c_int, ptr, c.POINTER(ptr))
name_of = bind("archive_entry_pathname", c.c_char_p, ptr)
size_of = bind("archive_entry_size", c.c_int64, ptr)
type_of = bind("archive_entry_filetype", c.c_uint, ptr)
hardlink = bind("archive_entry_hardlink", c.c_char_p, ptr)
encrypted = bind("archive_entry_is_encrypted", c.c_int, ptr)
read = bind("archive_read_data", c.c_ssize_t, ptr, ptr, c.c_size_t)
free = bind("archive_read_free", c.c_int, ptr)
error_message = bind("archive_error_string", c.c_char_p, ptr)


def inspect(source, destination, wanted=None, expected=None):
    archive = new()
    if not archive:
        raise MemoryError("Could not initialize the archive reader")

    def check(result):
        if result < 0:
            raise ValueError(os.fsdecode(error_message(archive) or b"Archive read error"))
        return result

    try:
        filters(archive)
        formats(archive)
        raw(archive)
        check(open_file(archive, os.fsencode(source), 65536))
        entries = []
        entry = ptr()
        index = 0
        while check(next_entry(archive, c.byref(entry))) != 1:
            # Raw format is useful for a single compressed stream. Plain bytes
            # with a misleading archive suffix belong in the byte inspector.
            if index == 0 and archive_format(archive) == 0x90000 and filter_count(archive) == 1:
                raise ValueError("Unrecognized archive container")
            name = (name_of(entry) or b"").decode("utf8", errors="replace")
            kind = type_of(entry)
            note = None
            # ar members carry permission bits but can omit the POSIX file type.
            regular = stat.S_ISREG(kind) or (kind == 0 and archive_format(archive) & 0xff0000 == 0x70000)
            if hardlink(entry) or not regular:
                note = "Link or special entry"
            elif encrypted(entry) == 1:
                note = "Encrypted entry"
            if wanted == index:
                if expected != name or note is not None:
                    raise ValueError("This entry is unavailable or has changed")
                buffer = c.create_string_buffer(65536)
                with open(destination, "wb") as output:
                    while count := check(read(archive, buffer, len(buffer))):
                        output.write(buffer.raw[:count])
                return
            if wanted is None and not stat.S_ISDIR(kind):
                entries.append(dict(index=index, name=name, size=max(size_of(entry), 0), note=note))
            index += 1
        if wanted is not None:
            raise ValueError("The selected entry is missing")
        with open(destination, "w", encoding="utf8") as output:
            json.dump(entries, output)
    finally:
        free(archive)


if __name__ == "__main__":
    args = sys.argv[1:]
    inspect(*args[:2], *([int(args[2]), args[3]] if len(args) == 4 else []))
