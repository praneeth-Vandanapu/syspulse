#!/usr/bin/env python3
# SysPulse - Multipurpose System Utility
# File   : scripts/md2docx.py
# Author : Praneeth
# License: MIT
#
# Converts the Markdown documents in docs/ into styled Word (.docx) files:
# title page, clickable table of contents, branded tables, shaded code blocks,
# page headers and "Page X of Y" footers.
#
# Usage:  python3 scripts/md2docx.py            (writes docs/word/*.docx)
# Needs:  python3-docx  (sudo apt install python3-docx  or  pip install python-docx)

import datetime
import pathlib
import re
import sys

from docx import Document
from docx.enum.section import WD_SECTION
from docx.enum.table import WD_TABLE_ALIGNMENT
from docx.enum.text import WD_ALIGN_PARAGRAPH, WD_BREAK
from docx.oxml import OxmlElement
from docx.oxml.ns import qn
from docx.shared import Cm, Pt, RGBColor

ROOT = pathlib.Path(__file__).resolve().parent.parent
DOCS = ROOT / "docs"
OUT = DOCS / "word"

VERSION = "0.6.0"
BODY_FONT = "Calibri"
MONO_FONT = "Consolas"
BRAND = RGBColor(0x1F, 0x6F, 0xEB)
DARK = RGBColor(0x1F, 0x29, 0x37)
MUTED = RGBColor(0x6B, 0x72, 0x80)
BRAND_HEX = "1F6FEB"
ZEBRA_HEX = "F3F6FB"
CODE_BG_HEX = "F4F5F7"
BORDER_HEX = "D0D7DE"

# source file -> (output file, friendly title, subtitle)
DOCUMENTS = {
    "USER_MANUAL.md": ("SysPulse User Manual.docx", "User Manual",
                       "How to use every feature of SysPulse"),
    "INSTALL.md": ("SysPulse Installation Guide.docx", "Installation Guide",
                   "Required modules, installation and building from source"),
    "ARCHITECTURE.md": ("SysPulse Architecture Guide.docx", "Architecture & Technology Guide",
                        "Concepts, modules, technologies and tools"),
    "ROADMAP.md": ("SysPulse Roadmap.docx", "Roadmap",
                   "Completed phases and planned work"),
}


# ---------------------------------------------------------------- helpers --

def slugify(text):
    """GitHub-style heading anchor."""
    s = re.sub(r"[^\w\- ]", "", strip_inline(text).strip().lower())
    return s.replace(" ", "-")


def bookmark_name(slug):
    # Word bookmarks: start with a letter, max 40 chars, [A-Za-z0-9_]
    return ("_" + re.sub(r"[^A-Za-z0-9]", "_", slug))[:40].replace("_", "h", 1)


def strip_inline(text):
    text = re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", text)
    return re.sub(r"[*`]", "", text)


def tidy(text):
    """Small wording/glyph substitutions so the text reads well in Word."""
    return text.replace("✅", "✔ Done").replace("🚧", "(in progress)")


def shade(element, hex_color):
    pr = element.get_or_add_tcPr() if element.tag.endswith("tc") else element.get_or_add_pPr()
    shd = OxmlElement("w:shd")
    shd.set(qn("w:val"), "clear")
    shd.set(qn("w:color"), "auto")
    shd.set(qn("w:fill"), hex_color)
    pr.append(shd)


def set_cell_margins(cell, top=60, bottom=60, left=100, right=100):
    tc_pr = cell._tc.get_or_add_tcPr()
    mar = OxmlElement("w:tcMar")
    for side, val in (("top", top), ("bottom", bottom), ("start", left), ("end", right)):
        el = OxmlElement(f"w:{side}")
        el.set(qn("w:w"), str(val))
        el.set(qn("w:type"), "dxa")
        mar.append(el)
    tc_pr.append(mar)


def table_borders(table, hex_color=BORDER_HEX):
    tbl_pr = table._tbl.tblPr
    borders = OxmlElement("w:tblBorders")
    for edge in ("top", "left", "bottom", "right", "insideH", "insideV"):
        el = OxmlElement(f"w:{edge}")
        el.set(qn("w:val"), "single")
        el.set(qn("w:sz"), "4")
        el.set(qn("w:space"), "0")
        el.set(qn("w:color"), hex_color)
        borders.append(el)
    tbl_pr.append(borders)


def repeat_header(row):
    tr_pr = row._tr.get_or_add_trPr()
    el = OxmlElement("w:tblHeader")
    el.set(qn("w:val"), "true")
    tr_pr.append(el)


def keep_with_next(paragraph):
    paragraph.paragraph_format.keep_with_next = True


def add_field(paragraph, instr, placeholder="1"):
    """Insert a Word field (PAGE, NUMPAGES ...)."""
    run = paragraph.add_run()
    begin = OxmlElement("w:fldChar")
    begin.set(qn("w:fldCharType"), "begin")
    run._r.append(begin)
    run = paragraph.add_run()
    it = OxmlElement("w:instrText")
    it.set(qn("xml:space"), "preserve")
    it.text = f" {instr} "
    run._r.append(it)
    run = paragraph.add_run()
    sep = OxmlElement("w:fldChar")
    sep.set(qn("w:fldCharType"), "separate")
    run._r.append(sep)
    paragraph.add_run(placeholder)
    run = paragraph.add_run()
    end = OxmlElement("w:fldChar")
    end.set(qn("w:fldCharType"), "end")
    run._r.append(end)


def add_bookmark(paragraph, name, ident):
    start = OxmlElement("w:bookmarkStart")
    start.set(qn("w:id"), str(ident))
    start.set(qn("w:name"), name)
    end = OxmlElement("w:bookmarkEnd")
    end.set(qn("w:id"), str(ident))
    paragraph._p.insert(0, start)
    paragraph._p.append(end)


def add_hyperlink(paragraph, text, url=None, anchor=None, size=None, bold=False, color=BRAND):
    link = OxmlElement("w:hyperlink")
    if url:
        r_id = paragraph.part.relate_to(
            url, "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink",
            is_external=True)
        link.set(qn("r:id"), r_id)
    else:
        link.set(qn("w:anchor"), anchor)
    run = OxmlElement("w:r")
    rpr = OxmlElement("w:rPr")
    c = OxmlElement("w:color")
    c.set(qn("w:val"), str(color))
    rpr.append(c)
    if url:
        u = OxmlElement("w:u")
        u.set(qn("w:val"), "single")
        rpr.append(u)
    if bold:
        rpr.append(OxmlElement("w:b"))
    if size:
        sz = OxmlElement("w:sz")
        sz.set(qn("w:val"), str(int(size * 2)))
        rpr.append(sz)
    run.append(rpr)
    t = OxmlElement("w:t")
    t.set(qn("xml:space"), "preserve")
    t.text = text
    run.append(t)
    link.append(run)
    paragraph._p.append(link)


# --------------------------------------------------------------- document --

class Converter:
    def __init__(self, src, out_name, title, subtitle):
        self.src = src
        self.out_name = out_name
        self.title = title
        self.subtitle = subtitle
        self.doc = Document()
        self.bookmark_id = 1
        self.headings = []        # (level, text, bookmark)
        self.anchor_map = {}      # slug -> bookmark
        self._setup_styles()
        self._setup_page()

    # ---- setup
    def _setup_styles(self):
        st = self.doc.styles
        normal = st["Normal"]
        normal.font.name = BODY_FONT
        normal.font.size = Pt(10.5)
        normal.font.color.rgb = DARK
        normal.element.rPr.rFonts.set(qn("w:eastAsia"), BODY_FONT)
        normal.paragraph_format.space_after = Pt(6)
        normal.paragraph_format.line_spacing = 1.15
        for lvl, size, color, before in ((1, 18, BRAND, 18), (2, 14, DARK, 14), (3, 12, DARK, 10)):
            h = st[f"Heading {lvl}"]
            h.font.name = BODY_FONT
            h.element.rPr.rFonts.set(qn("w:asciiTheme"), "") if False else None
            rfonts = h.element.rPr.find(qn("w:rFonts"))
            if rfonts is not None:
                for attr in ("w:asciiTheme", "w:hAnsiTheme", "w:eastAsiaTheme", "w:cstheme"):
                    rfonts.attrib.pop(qn(attr), None)
            h.font.size = Pt(size)
            h.font.bold = True
            h.font.color.rgb = color
            h.paragraph_format.space_before = Pt(before)
            h.paragraph_format.space_after = Pt(6)
            h.paragraph_format.keep_with_next = True
        for name in ("List Bullet", "List Bullet 2", "List Bullet 3"):
            st[name].font.name = BODY_FONT
            st[name].paragraph_format.space_after = Pt(3)

    def _setup_page(self):
        sec = self.doc.sections[0]
        sec.page_height, sec.page_width = Cm(29.7), Cm(21.0)
        for side in ("left_margin", "right_margin"):
            setattr(sec, side, Cm(2.0))
        sec.top_margin, sec.bottom_margin = Cm(2.2), Cm(2.0)
        sec.different_first_page_header_footer = True

        hp = sec.header.paragraphs[0]
        hp.alignment = WD_ALIGN_PARAGRAPH.RIGHT
        r = hp.add_run(f"SysPulse  ·  {self.title}")
        r.font.size, r.font.color.rgb = Pt(8.5), MUTED

        fp = sec.footer.paragraphs[0]
        fp.alignment = WD_ALIGN_PARAGRAPH.CENTER
        for part in ("Page ", ("PAGE",), " of ", ("NUMPAGES",)):
            if isinstance(part, tuple):
                add_field(fp, part[0])
            else:
                fp.add_run(part)
        for r in fp.runs:
            r.font.size, r.font.color.rgb = Pt(8.5), MUTED

    @property
    def text_width(self):
        sec = self.doc.sections[0]
        return sec.page_width - sec.left_margin - sec.right_margin

    # ---- inline markdown
    INLINE = re.compile(r"(\*\*[^*]+\*\*|`[^`]+`|\[[^\]]+\]\([^)]+\)|(?<![\w*])\*[^*\s][^*]*\*(?![\w*]))")

    def add_inline(self, paragraph, text, size=None, bold=False, color=None):
        text = tidy(text)
        pos = 0
        for m in self.INLINE.finditer(text):
            if m.start() > pos:
                self._run(paragraph, text[pos:m.start()], size=size, bold=bold, color=color)
            tok = m.group(0)
            if tok.startswith("**"):
                self.add_inline(paragraph, tok[2:-2], size=size, bold=True, color=color)
            elif tok.startswith("`"):
                r = self._run(paragraph, tok[1:-1], size=(size or 10.5) - 1, bold=bold, color=color)
                r.font.name = MONO_FONT
                r._r.get_or_add_rPr().get_or_add_rFonts().set(qn("w:hAnsi"), MONO_FONT)
                rpr = r._r.get_or_add_rPr()
                shd = OxmlElement("w:shd")
                shd.set(qn("w:val"), "clear")
                shd.set(qn("w:fill"), "EEF1F5")
                rpr.append(shd)
            elif tok.startswith("["):
                label, target = re.match(r"\[([^\]]+)\]\(([^)]+)\)", tok).groups()
                self._link(paragraph, strip_inline(label), target, size)
            else:
                r = self._run(paragraph, tok[1:-1], size=size, bold=bold, color=color)
                r.italic = True
            pos = m.end()
        if pos < len(text):
            self._run(paragraph, text[pos:], size=size, bold=bold, color=color)

    def _run(self, paragraph, text, size=None, bold=False, color=None):
        r = paragraph.add_run(text)
        if size:
            r.font.size = Pt(size)
        if bold:
            r.bold = True
        if color is not None:
            r.font.color.rgb = color
        return r

    def _link(self, paragraph, label, target, size):
        if target.startswith("#"):
            # resolved after all headings are known (see finish)
            self.pending_links.append((paragraph, label, target[1:], size))
            add_hyperlink(paragraph, label, anchor="__PENDING__" + target[1:], size=size)
        elif target.startswith(("http://", "https://", "mailto:")):
            add_hyperlink(paragraph, label, url=target, size=size)
        else:
            # link to another doc: name the Word document instead of the .md file
            md = pathlib.Path(target.split("#")[0]).name
            if md in DOCUMENTS:
                friendly = DOCUMENTS[md][0][:-5]
                if label.lower().endswith(".md"):
                    self._run(paragraph, f"the “{friendly}”", size=size)
                else:
                    self._run(paragraph, f"{label} (see the “{friendly}”)", size=size)
            else:
                self._run(paragraph, label, size=size)

    # ---- blocks
    def heading(self, level, text):
        text = tidy(text)
        word_level = max(1, level - 1)       # "## x" is a top-level chapter
        p = self.doc.add_heading(level=word_level)
        self.add_inline(p, text)
        bm = f"sec{self.bookmark_id}"
        add_bookmark(p, bm, self.bookmark_id)
        self.bookmark_id += 1
        self.headings.append((word_level, strip_inline(text), bm))
        self.anchor_map[slugify(text.replace("✔ Done", "✅"))] = bm
        self.anchor_map[slugify(text)] = bm

    def paragraph(self, text):
        p = self.doc.add_paragraph()
        self.add_inline(p, text)
        if text.rstrip().endswith(":"):
            keep_with_next(p)

    def list_item(self, text, depth, number=None, checked=None):
        indent = Cm(0.63 + 0.63 * depth)
        if number is None and checked is None:
            style = ("List Bullet", "List Bullet 2", "List Bullet 3")[min(depth, 2)]
            p = self.doc.add_paragraph(style=style)
        else:
            p = self.doc.add_paragraph()
            pf = p.paragraph_format
            pf.left_indent = indent
            pf.first_line_indent = Cm(-0.63)
            pf.space_after = Pt(3)
            pf.tab_stops.add_tab_stop(indent)
            if checked is not None:
                r = p.add_run(("☑" if checked else "☐") + "\t")
                r.font.color.rgb = RGBColor(0x1A, 0x7F, 0x37) if checked else MUTED
            else:
                r = p.add_run(f"{number}.\t")
                r.bold = True
                r.font.color.rgb = BRAND
        self.add_inline(p, text)

    def quote(self, text):
        t = self.doc.add_table(rows=1, cols=1)
        cell = t.rows[0].cells[0]
        shade(cell._tc, "EAF2FF")
        set_cell_margins(cell, 100, 100, 160, 160)
        self.add_inline(cell.paragraphs[0], text)
        self.doc.add_paragraph().paragraph_format.space_after = Pt(0)

    def code(self, lines, lang):
        longest = max((len(line) for line in lines), default=0)
        size = 9 if longest <= 84 else (8 if longest <= 96 else 7)
        t = self.doc.add_table(rows=1, cols=1)
        t.alignment = WD_TABLE_ALIGNMENT.CENTER
        table_borders(t, "E1E4E8")
        cell = t.rows[0].cells[0]
        shade(cell._tc, CODE_BG_HEX)
        set_cell_margins(cell, 100, 100, 140, 140)
        p = cell.paragraphs[0]
        p.paragraph_format.space_after = Pt(0)
        p.paragraph_format.line_spacing = 1.0
        if lang in ("bash", "sh", "shell"):
            cap = p.add_run("Terminal\n")
            cap.font.size, cap.font.color.rgb, cap.bold = Pt(7.5), MUTED, True
        for i, line in enumerate(lines):
            is_comment = lang in ("bash", "sh", "shell") and line.lstrip().startswith("#")
            r = p.add_run(line)
            r.font.name = MONO_FONT
            r._r.get_or_add_rPr().get_or_add_rFonts().set(qn("w:hAnsi"), MONO_FONT)
            r.font.size = Pt(size)
            r.font.color.rgb = RGBColor(0x5A, 0x8A, 0x3C) if is_comment else DARK
            if i < len(lines) - 1:
                r.add_break(WD_BREAK.LINE)
        spacer = self.doc.add_paragraph()
        spacer.paragraph_format.space_after = Pt(2)

    def table(self, rows):
        header, body = rows[0], rows[1:]
        ncols = len(header)
        t = self.doc.add_table(rows=1 + len(body), cols=ncols)
        t.alignment = WD_TABLE_ALIGNMENT.CENTER
        table_borders(t)
        # column widths proportional to content length (bounded)
        lens = [max(len(strip_inline(r[i])) if i < len(r) else 0 for r in rows) for i in range(ncols)]
        weights = [min(max(n, 6), 60) for n in lens]
        total = sum(weights)
        widths = [int(self.text_width * w / total) for w in weights]
        for ri, row in enumerate([header] + body):
            tr = t.rows[ri]
            if ri == 0:
                repeat_header(tr)
            for ci in range(ncols):
                cell = tr.cells[ci]
                cell.width = widths[ci]
                set_cell_margins(cell)
                text = row[ci] if ci < len(row) else ""
                p = cell.paragraphs[0]
                p.paragraph_format.space_after = Pt(0)
                p.paragraph_format.line_spacing = 1.05
                if ri == 0:
                    shade(cell._tc, BRAND_HEX)
                    self.add_inline(p, text, size=9.5, bold=True, color=RGBColor(0xFF, 0xFF, 0xFF))
                else:
                    if ri % 2 == 0:
                        shade(cell._tc, ZEBRA_HEX)
                    # first column reads as a label
                    self.add_inline(p, text, size=9.5, bold=(ci == 0 and ncols > 1 and len(text) < 40))
        spacer = self.doc.add_paragraph()
        spacer.paragraph_format.space_after = Pt(2)

    # ---- title page and contents
    def title_page(self, meta_line):
        d = self.doc
        for _ in range(7):
            d.add_paragraph()
        p = d.add_paragraph()
        r = p.add_run("SysPulse")
        r.font.size, r.bold, r.font.color.rgb = Pt(40), True, BRAND
        p = d.add_paragraph()
        r = p.add_run("Multipurpose System Utility for Ubuntu")
        r.font.size, r.font.color.rgb = Pt(13), MUTED
        # accent rule
        p = d.add_paragraph()
        p_pr = p._p.get_or_add_pPr()
        bdr = OxmlElement("w:pBdr")
        bottom = OxmlElement("w:bottom")
        for k, v in (("w:val", "single"), ("w:sz", "18"), ("w:space", "1"), ("w:color", BRAND_HEX)):
            bottom.set(qn(k), v)
        bdr.append(bottom)
        p_pr.append(bdr)
        p = d.add_paragraph()
        p.paragraph_format.space_before = Pt(18)
        r = p.add_run(self.title)
        r.font.size, r.bold, r.font.color.rgb = Pt(26), True, DARK
        p = d.add_paragraph()
        r = p.add_run(self.subtitle)
        r.font.size, r.font.color.rgb = Pt(13), MUTED
        for _ in range(9):
            d.add_paragraph()
        info = [("Author", "Praneeth"), ("Version", VERSION),
                ("Date", datetime.date.today().strftime("%d %B %Y")), ("License", "MIT (open source)")]
        for k, v in info:
            p = d.add_paragraph()
            p.paragraph_format.space_after = Pt(2)
            r = p.add_run(f"{k}:  ")
            r.font.color.rgb, r.font.size = MUTED, Pt(10.5)
            r = p.add_run(v)
            r.bold, r.font.size = True, Pt(10.5)
        d.add_page_break()
        self.toc_anchor = d.add_paragraph()   # contents are filled in at the end

    def build_toc(self):
        """A real, clickable contents list (no 'update field' prompt needed)."""
        anchor = self.toc_anchor
        h = self.doc.add_heading("Contents", level=1)
        anchor._p.addprevious(h._p)
        for level, text, bm in self.headings:
            if level > 2:
                continue
            p = self.doc.add_paragraph()
            pf = p.paragraph_format
            pf.left_indent = Cm(0 if level == 1 else 0.8)
            pf.space_after = Pt(4 if level == 1 else 1)
            add_hyperlink(p, text, anchor=bm, size=11 if level == 1 else 10,
                          bold=(level == 1), color=DARK if level == 1 else BRAND)
            anchor._p.addprevious(p._p)
        brk = self.doc.add_paragraph()
        brk.add_run().add_break(WD_BREAK.PAGE)
        anchor._p.addprevious(brk._p)
        anchor._p.getparent().remove(anchor._p)

    def resolve_links(self):
        for el in self.doc.element.body.iter(qn("w:hyperlink")):
            a = el.get(qn("w:anchor"))
            if a and a.startswith("__PENDING__"):
                slug = a[len("__PENDING__"):]
                el.set(qn("w:anchor"), self.anchor_map.get(slug, "sec1"))

    # ---- main parse
    def convert(self):
        self.pending_links = []
        text = self.src.read_text(encoding="utf-8")
        text = re.sub(r"<!--.*?-->", "", text, flags=re.S).strip("\n")
        lines = text.split("\n")

        i = 0
        meta = ""
        # H1 + metadata line become the title page
        while i < len(lines) and not lines[i].startswith("# "):
            i += 1
        i += 1
        while i < len(lines) and (not lines[i].strip() or lines[i].startswith("**Author")):
            if lines[i].startswith("**Author"):
                meta = lines[i]
            i += 1
        self.title_page(meta)

        skipping_contents = False
        para = []

        def flush():
            if para:
                self.paragraph(" ".join(s.strip() for s in para))
                para.clear()

        while i < len(lines):
            line = lines[i]
            stripped = line.strip()

            heading = re.match(r"^(#{2,6})\s+(.*)$", line)
            if heading:
                flush()
                level, htext = len(heading.group(1)), heading.group(2).strip()
                skipping_contents = htext.lower() in ("contents", "table of contents")
                if not skipping_contents:
                    self.heading(level, htext)
                i += 1
                continue
            if skipping_contents:
                i += 1
                continue

            if stripped.startswith("```"):
                flush()
                lang = stripped[3:].strip()
                indent = len(line) - len(line.lstrip())
                block = []
                i += 1
                while i < len(lines) and not lines[i].strip().startswith("```"):
                    block.append(lines[i][indent:] if lines[i][:indent].isspace() else lines[i])
                    i += 1
                self.code(block, lang)
                i += 1
                continue

            if stripped.startswith("|") and i + 1 < len(lines) and re.match(r"^\s*\|[\s:|-]+\|\s*$", lines[i + 1]):
                flush()
                rows = []
                while i < len(lines) and lines[i].strip().startswith("|"):
                    if not re.match(r"^\s*\|[\s:|-]+\|\s*$", lines[i]):
                        cells = re.split(r"(?<!\\)\|", lines[i].strip()[1:-1])
                        rows.append([c.strip().replace("\\|", "|") for c in cells])
                    i += 1
                self.table(rows)
                continue

            if re.match(r"^-{3,}\s*$", stripped) or not stripped:
                flush()
                i += 1
                continue

            if stripped.startswith(">"):
                flush()
                q = []
                while i < len(lines) and lines[i].strip().startswith(">"):
                    q.append(lines[i].strip()[1:].strip())
                    i += 1
                self.quote(" ".join(q))
                continue

            m = re.match(r"^(\s*)([-*]|\d+\.)\s+(.*)$", line)
            if m:
                flush()
                spaces, marker, body = m.groups()
                depth = len(spaces.replace("\t", "    ")) // 2
                depth = min(depth // 1 if marker[0].isdigit() else depth, 3)
                # continuation lines (indented, not a new item)
                i += 1
                while (i < len(lines) and lines[i].strip() and lines[i].startswith(" ")
                       and not re.match(r"^\s*([-*]|\d+\.)\s+", lines[i])
                       and not lines[i].strip().startswith(("```", "|"))):
                    body += " " + lines[i].strip()
                    i += 1
                checked = None
                cb = re.match(r"^\[([ xX])\]\s+(.*)$", body)
                if cb:
                    checked, body = cb.group(1).lower() == "x", cb.group(2)
                number = marker[:-1] if marker[0].isdigit() else None
                if number:
                    depth = min(len(spaces) // 3, 3)
                elif depth and len(spaces) == 3:
                    depth = 1   # "   * x" under a numbered item
                self.list_item(body, depth, number=number, checked=checked)
                continue

            para.append(line)
            i += 1
        flush()

        self.build_toc()
        self.resolve_links()
        cp = self.doc.core_properties
        cp.author = "Praneeth"
        cp.title = f"SysPulse — {self.title}"
        cp.subject = self.subtitle
        cp.keywords = "SysPulse, Ubuntu, system monitor"
        OUT.mkdir(exist_ok=True)
        out = OUT / self.out_name
        self.doc.save(out)
        return out


def main():
    names = sys.argv[1:] or list(DOCUMENTS)
    for name in names:
        out_name, title, subtitle = DOCUMENTS[name]
        out = Converter(DOCS / name, out_name, title, subtitle).convert()
        print(f"wrote {out.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
