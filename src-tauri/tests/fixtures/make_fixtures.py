"""Writes minimal, valid PDFs used by the Rust tests. Run: python make_fixtures.py"""
import os

def build(pages):
    objs = []
    def add(body):
        objs.append(body)
        return len(objs)
    font = add(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>")
    pages_id = len(objs) + 1 + 2 * len(pages)  # placeholder, fixed below
    kids = []
    page_objs = []
    for text, rotate in pages:
        lines = text.split("\n")
        ops = ["BT /F1 12 Tf 72 720 Td 14 TL"]
        for ln in lines:
            ops.append("(%s) Tj T*" % ln.replace("\\", "\\\\").replace("(", r"\(").replace(")", r"\)"))
        ops.append("ET")
        stream = ("\n".join(ops) if text else "").encode("latin-1")
        content = add(b"<< /Length %d >>\nstream\n" % len(stream) + stream + b"\nendstream")
        page_objs.append((content, rotate))
    pages_id = len(objs) + len(page_objs) + 1
    for content, rotate in page_objs:
        rot = b" /Rotate %d" % rotate if rotate else b""
        kids.append(add(b"<< /Type /Page /Parent %d 0 R /MediaBox [0 0 612 792]%s /Resources << /Font << /F1 %d 0 R >> >> /Contents %d 0 R >>" % (pages_id, rot, font, content)))
    assert add(b"<< /Type /Pages /Kids [%s] /Count %d >>" % (b" ".join(b"%d 0 R" % k for k in kids), len(kids))) == pages_id
    info = add(b"<< /Title (Evidence Linking in Local Research Assistants) /Author (Test Author) /CreationDate (D:20240115000000Z) >>")
    catalog = add(b"<< /Type /Catalog /Pages %d 0 R >>" % pages_id)
    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets = []
    for i, body in enumerate(objs, 1):
        offsets.append(len(out))
        out += b"%d 0 obj\n" % i + body + b"\nendobj\n"
    xref = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1)
    for off in offsets:
        out += b"%010d 00000 n \n" % off
    out += b"trailer\n<< /Size %d /Root %d 0 R /Info %d 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objs) + 1, catalog, info, xref)
    return bytes(out)

here = os.path.dirname(os.path.abspath(__file__))
page1 = "Evidence Linking in Local Research Assistants\nRetrieval augmented generation grounds answers in sources.\nEvery claim should cite the page it came from."
page2 = "Results\nHybrid retrieval improved recall at k = 20 by 4.1 points."
with open(os.path.join(here, "two-pages.pdf"), "wb") as f:
    f.write(build([(page1, 0), (page2, 90)]))
with open(os.path.join(here, "blank-scan.pdf"), "wb") as f:
    f.write(build([("", 0), ("", 0)]))
print("ok")
