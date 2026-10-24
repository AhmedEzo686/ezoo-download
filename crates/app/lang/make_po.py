"""Rebuild lang/<code>/LC_MESSAGES/swiftget.po from the @tr("...") texts in ui/*.slint.

Arabic comes from ar_ui.py, the other languages from ui/<code>.json
(English text -> translation, written the way the .slint files write it).
"""
import json, os, re, sys

here = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, here)
from ar_ui import AR

texts = []
for f in ["common.slint", "app.slint", "dialogs.slint"]:
    src = open(os.path.join(here, "..", "ui", f), encoding="utf-8").read()
    for m in re.finditer(r'@tr\("((?:[^"\\]|\\.)*)"', src):
        if m.group(1) not in texts:
            texts.append(m.group(1))

tables = {"ar": AR}
for name in sorted(os.listdir(os.path.join(here, "ui"))):
    if name.endswith(".json"):
        tables[name[:-5]] = json.load(open(os.path.join(here, "ui", name), encoding="utf-8"))

for code, table in tables.items():
    out = ['msgid ""', 'msgstr ""', '"Content-Type: text/plain; charset=UTF-8\\n"',
           f'"Language: {code}\\n"', '"Plural-Forms: nplurals=2; plural=(n != 1);\\n"', ""]
    for t in texts:
        out += [f'msgid "{t}"', f'msgstr "{table.get(t, t)}"', ""]
    path = os.path.join(here, code, "LC_MESSAGES")
    os.makedirs(path, exist_ok=True)
    open(os.path.join(path, "swiftget.po"), "w", encoding="utf-8").write("\n".join(out))
    missing = [t for t in texts if t not in table]
    print(f"{code}: {len(texts)} texts, {len(missing)} without translation")
    for t in missing:
        print("  missing:", t)
