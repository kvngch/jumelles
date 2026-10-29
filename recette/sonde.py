"""Sonde du runner : WebView2 par le protocole DevTools, dialogue de dossier par UI Automation."""

import os
import subprocess
import sys
import time

from playwright.sync_api import sync_playwright
from pywinauto import Desktop, findwindows

exe = sys.argv[1]
dossier = sys.argv[2]
env = dict(os.environ, WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222")
p = subprocess.Popen([exe], env=env)
with sync_playwright() as pw:
    for _ in range(60):
        try:
            b = pw.chromium.connect_over_cdp("http://127.0.0.1:9222")
            break
        except Exception as e:
            print("attente", e)
            time.sleep(1)
    page = b.contexts[0].pages[0]
    print("url", page.url)
    print("langue", page.evaluate("navigator.language"), page.evaluate("navigator.languages"))
    print("ua", page.evaluate("navigator.userAgent"))
    print("taille", page.evaluate("[innerWidth, innerHeight, devicePixelRatio]"))
    page.screenshot(path="sonde-accueil.png")

    page.click("[data-action=ajouter]")
    hs = []
    for _ in range(50):
        hs = findwindows.find_windows(process=p.pid, class_name="#32770")
        if hs:
            break
        time.sleep(0.2)
    print("dialogues", hs)
    d = Desktop(backend="uia").window(handle=hs[0])
    d.wait("ready", timeout=10)
    print("titre", d.window_text())
    for e in d.descendants():
        info = e.element_info
        if info.control_type in ("Edit", "Button", "ComboBox"):
            print(" ", info.control_type, repr(info.name), info.automation_id)
    champ = d.child_window(auto_id="1152", control_type="Edit")
    champ.set_edit_text(dossier)
    d.child_window(auto_id="1", control_type="Button").invoke()
    time.sleep(2)
    print("encore ouvert", bool(findwindows.find_windows(process=p.pid, class_name="#32770")))
    print("dossiers", page.evaluate("etat.dossiers"))

    # depot simule par l'evenement de Tauri
    page.evaluate(
        "p => window.__TAURI__.event.emit('tauri://drag-drop', {paths: [p], position: {x: 10, y: 10}})",
        dossier + "\\sous",
    )
    time.sleep(1)
    print("apres depot", page.evaluate("etat.dossiers"))

    # langue forcee avant le chargement de main.js
    page.add_init_script("Object.defineProperty(Navigator.prototype, 'language', {get: () => 'fr-FR'})")
    page.reload()
    page.wait_for_selector(".accueil")
    print("apres rechargement", page.evaluate("[navigator.language, T.code, etat.dossiers]"))
    page.screenshot(path="sonde-fr.png")
    b.close()
p.kill()
