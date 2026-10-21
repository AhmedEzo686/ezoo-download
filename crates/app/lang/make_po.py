"""Rebuild lang/ar/LC_MESSAGES/swiftget.po from the @tr("...") texts in ui/*.slint."""
import os, re, sys
here = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, here)
from ar_ui import AR

texts = []
for f in ["common.slint", "app.slint", "dialogs.slint"]:
    src = open(os.path.join(here, "..", "ui", f), encoding="utf-8").read()
    for m in re.finditer(r'@tr\("((?:[^"\\]|\\.)*)"', src):
        if m.group(1) not in texts:
            texts.append(m.group(1))
missing = [t for t in texts if t not in AR]
out = ['msgid ""', 'msgstr ""', '"Content-Type: text/plain; charset=UTF-8\\n"',
       '"Language: ar\\n"', '"Plural-Forms: nplurals=2; plural=(n != 1);\\n"', ""]
for t in texts:
    out += [f'msgid "{t}"', f'msgstr "{AR.get(t, t)}"', ""]
path = os.path.join(here, "ar", "LC_MESSAGES")
os.makedirs(path, exist_ok=True)
open(os.path.join(path, "swiftget.po"), "w", encoding="utf-8").write("\n".join(out))
print(f"{len(texts)} texts, {len(missing)} without Arabic")
for t in missing:
    print("  missing:", t)
