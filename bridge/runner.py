#!/usr/bin/env python3
import sys
import os
import json
import traceback
import importlib
import importlib.util
import threading
import gi

gi.require_version("GLib", "2.0")
gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import GLib, Gtk, Adw, Gdk
from loguru import logger

# Ensure GTK & Libadwaita are initialized cleanly in the bridge process
try:
    Adw.init()
except Exception:
    pass

# Add bridge root to Python path so `src.*`, `GtkHelper.*`, `locales.*`, `globals`, `streamcontroller_plugin_tools` resolve
BRIDGE_DIR = os.path.dirname(os.path.abspath(__file__))
if BRIDGE_DIR not in sys.path:
    sys.path.insert(0, BRIDGE_DIR)

# Optional fallback: if rpyc is available inside user's StreamController flatpak runtime, expose only pure-python packages if needed
import globals as gl
from src.backend.DeckManagement.InputIdentifier import Input
from src.backend.DeckManagement.DeckController import DeckController
from src.backend.PageManagement.Page import Page
from src.backend.PluginManager.PluginBase import PluginBase
from src.backend.PluginManager.ActionCore import ActionCore
from src.backend.PluginManager.ActionInputSupport import ActionInputSupport


def find_child_by_type(root, target_type):
    if root is None:
        return None
    stack = [root]
    while stack:
        curr = stack.pop()
        if isinstance(curr, target_type):
            return curr
        c = curr.get_first_child() if hasattr(curr, "get_first_child") else None
        children = []
        while c is not None:
            children.append(c)
            c = c.get_next_sibling()
        for ch in reversed(children):
            stack.append(ch)
    return None


def extract_gtk_image_path_or_name(widget) -> tuple[str, str]:
    """Extract (file_path, icon_name) from a Gtk.Image or similar widget."""
    if widget is None:
        return ("", "")
    # Check custom attribute or Gtk.Image file / paintable / icon-name
    try:
        if hasattr(widget, "get_property"):
            file_prop = widget.get_property("file")
            if file_prop:
                return (str(file_prop), "")
    except Exception:
        pass
    try:
        if hasattr(widget, "get_icon_name"):
            iname = widget.get_icon_name()
            if iname:
                return ("", str(iname))
    except Exception:
        pass
    try:
        if hasattr(widget, "get_gicon"):
            gicon = widget.get_gicon()
            if gicon and hasattr(gicon, "get_file"):
                f = gicon.get_file()
                if f:
                    return (f.get_path() or "", "")
    except Exception:
        pass
    return ("", "")


def extract_model_strings(model) -> list[str]:
    """Extract display strings from Gtk.StringList, Gio.ListStore, or Gtk.ListStore."""
    if model is None:
        return []
    opts = []
    # 1. Gtk.StringList (has get_string)
    if hasattr(model, "get_n_items") and hasattr(model, "get_string"):
        n = model.get_n_items()
        for i in range(n):
            try:
                opts.append(str(model.get_string(i) or ""))
            except Exception:
                opts.append("")
        return opts
    # 2. Gio.ListModel / Gio.ListStore (e.g. AppItem, BaseComboRowItem, ItemListComboRowListItem, SearchComboRowItem)
    if hasattr(model, "get_n_items") and hasattr(model, "get_item"):
        n = model.get_n_items()
        for i in range(n):
            try:
                item = model.get_item(i)
                if item is None:
                    opts.append("")
                    continue
                label = None
                for attr in ("title", "display_label", "label", "name"):
                    if hasattr(item, attr):
                        val = getattr(item, attr)
                        if isinstance(val, str) and val:
                            label = val
                            break
                if label is None:
                    if hasattr(item, "get_string"):
                        label = item.get_string()
                    else:
                        label = str(item)
                opts.append(str(label or ""))
            except Exception:
                opts.append("")
        return opts
    # 3. Legacy Gtk.TreeModel / Gtk.ListStore (used by Gtk.ComboBox in OBSPlugin, OSPlugin, etc.)
    if hasattr(model, "get_iter_first") and hasattr(model, "iter_next"):
        it = model.get_iter_first()
        while it is not None:
            try:
                val = model.get_value(it, 0)
                opts.append(str(val if val is not None else ""))
            except Exception:
                opts.append("")
            it = model.iter_next(it)
        return opts
    return opts


def flatten_config_widgets(raw_widgets) -> list:
    """
    Recursively flatten Adw.PreferencesGroup, Adw.PreferencesPage, Gtk.Box, Gtk.ListBox,
    Adw.ExpanderRow, etc. into an ordered list of leaf/row widgets for Ballista UI serialization.
    """
    if raw_widgets is None:
        return []
    if not isinstance(raw_widgets, (list, tuple)):
        raw_widgets = [raw_widgets]

    collected = []
    seen = set()

    def walk(w):
        if w is None or w in seen:
            return
        seen.add(w)

        # ExpanderRow: emit itself (as SwitchRow if show_enable_switch else ActionRow header), AND walk its inner rows
        if isinstance(w, Adw.ExpanderRow):
            show_sw = hasattr(w, "get_show_enable_switch") and bool(w.get_show_enable_switch())
            has_label = bool((w.get_title() if hasattr(w, "get_title") else "") or (w.get_subtitle() if hasattr(w, "get_subtitle") else ""))
            if show_sw or has_label:
                collected.append(w)
            # Walk child rows added to expander
            if hasattr(w, "_rows") and isinstance(w._rows, list):
                for r in w._rows:
                    walk(r)
            else:
                # Discover child ListBox rows inside GTK4 Adw.ExpanderRow
                lb = find_child_by_type(w, Gtk.ListBox)
                if lb is not None:
                    c = lb.get_first_child()
                    while c is not None:
                        if c is not w and isinstance(c, (Adw.PreferencesRow, Gtk.ListBoxRow)):
                            # Avoid re-walking the header row itself
                            if not isinstance(c, Adw.ExpanderRow):
                                walk(c)
                        c = c.get_next_sibling()
            return

        # Direct leaf PreferenceRows
        if isinstance(w, (Adw.ComboRow, Adw.SwitchRow, Adw.PasswordEntryRow, Adw.EntryRow, Adw.SpinRow, Adw.ActionRow)):
            collected.append(w)
            return

        # Standalone interactive controls inside custom Gtk.Box areas
        if isinstance(w, (Gtk.ColorButton, Gtk.ColorDialogButton, Gtk.Scale, Gtk.Switch, Gtk.SpinButton,
                          Gtk.PasswordEntry, Gtk.Entry, Gtk.DropDown, Gtk.ComboBox, Gtk.CheckButton, Gtk.Button)):
            collected.append(w)
            return

        # Containers: Adw.PreferencesPage, Adw.PreferencesGroup, Gtk.Box, Gtk.ListBox, Gtk.Frame, Gtk.ScrolledWindow, etc.
        # Note: Adw.PreferencesGroup stores its rows inside an internal Gtk.ListBox child!
        if hasattr(w, "get_first_child"):
            c = w.get_first_child()
            while c is not None:
                walk(c)
                c = c.get_next_sibling()

    for item in raw_widgets:
        walk(item)

    return collected


def serialize_widget_row(row, idx: int) -> dict:
    title = ""
    subtitle = ""
    visible = True
    sensitive = True

    try:
        if hasattr(row, "get_title"):
            title = row.get_title() or ""
    except Exception:
        pass
    try:
        if hasattr(row, "get_subtitle"):
            subtitle = row.get_subtitle() or ""
    except Exception:
        pass
    try:
        if hasattr(row, "get_visible"):
            visible = bool(row.get_visible())
    except Exception:
        pass
    try:
        if hasattr(row, "get_sensitive"):
            sensitive = bool(row.get_sensitive())
    except Exception:
        pass

    # 1. Adw.ComboRow (includes GtkHelper.ComboRow, ToggleRow, SearchComboRow, ItemListComboRow)
    if isinstance(row, Adw.ComboRow):
        opts = extract_model_strings(row.get_model())
        sel = row.get_selected()
        if sel == 4294967295 or sel < 0:
            sel = 0
        return {
            "index": idx,
            "type": "ComboRow",
            "title": title,
            "subtitle": subtitle,
            "visible": visible,
            "sensitive": sensitive,
            "options": opts,
            "selected": int(sel),
        }

    # 2. Adw.SwitchRow
    if isinstance(row, Adw.SwitchRow):
        return {
            "index": idx,
            "type": "SwitchRow",
            "title": title,
            "subtitle": subtitle,
            "visible": visible,
            "sensitive": sensitive,
            "active": bool(row.get_active()),
        }

    # 3. Adw.ExpanderRow (SwitchRow only when show_enable_switch=True; otherwise header/status ActionRow)
    if isinstance(row, Adw.ExpanderRow):
        show_sw = hasattr(row, "get_show_enable_switch") and bool(row.get_show_enable_switch())
        if show_sw:
            return {
                "index": idx,
                "type": "SwitchRow",
                "title": title,
                "subtitle": subtitle,
                "visible": visible,
                "sensitive": sensitive,
                "active": bool(row.get_enable_expansion()),
            }
        return {
            "index": idx,
            "type": "ActionRow",
            "title": title,
            "subtitle": subtitle,
            "visible": visible,
            "sensitive": sensitive,
        }

    # 4. Adw.PasswordEntryRow (MUST check BEFORE Adw.EntryRow because PasswordEntryRow subclasses EntryRow!)
    if isinstance(row, Adw.PasswordEntryRow):
        editable = bool(row.get_editable()) if hasattr(row, "get_editable") else True
        return {
            "index": idx,
            "type": "PasswordRow",
            "title": title,
            "subtitle": subtitle,
            "visible": visible,
            "sensitive": sensitive,
            "editable": editable,
            "text": row.get_text() or "",
        }

    # 5. Adw.EntryRow
    if isinstance(row, Adw.EntryRow):
        editable = bool(row.get_editable()) if hasattr(row, "get_editable") else True
        return {
            "index": idx,
            "type": "EntryRow",
            "title": title,
            "subtitle": subtitle,
            "visible": visible,
            "sensitive": sensitive,
            "editable": editable,
            "text": row.get_text() or "",
        }

    # 6. Adw.SpinRow
    if isinstance(row, Adw.SpinRow):
        adj = row.get_adjustment()
        digits = int(row.get_digits()) if hasattr(row, "get_digits") else 0
        return {
            "index": idx,
            "type": "SpinRow",
            "title": title,
            "subtitle": subtitle,
            "visible": visible,
            "sensitive": sensitive,
            "value": float(row.get_value()),
            "min": float(adj.get_lower()) if adj else 0.0,
            "max": float(adj.get_upper()) if adj else 100.0,
            "step": float(adj.get_step_increment()) if adj else 1.0,
            "digits": digits,
        }

    # 7. Adw.ActionRow (inspect embedded suffix control: Color, Scale, ComboBox, DropDown, Switch, SpinButton, Entry, Button)
    if isinstance(row, Adw.ActionRow):
        color_btn = find_child_by_type(row, Gtk.ColorButton) or find_child_by_type(row, Gtk.ColorDialogButton)
        if color_btn is not None:
            rgba = color_btn.get_rgba()
            hex_c = f"#{int(round(rgba.red * 255)):02X}{int(round(rgba.green * 255)):02X}{int(round(rgba.blue * 255)):02X}"
            return {
                "index": idx,
                "type": "ColorRow",
                "title": title,
                "subtitle": subtitle,
                "visible": visible,
                "sensitive": sensitive and bool(color_btn.get_sensitive()),
                "color": hex_c,
            }

        scale = find_child_by_type(row, Gtk.Scale)
        if scale is not None:
            adj = scale.get_adjustment()
            digits = int(scale.get_digits()) if hasattr(scale, "get_digits") else 0
            return {
                "index": idx,
                "type": "ScaleRow",
                "title": title,
                "subtitle": subtitle,
                "visible": visible,
                "sensitive": sensitive and bool(scale.get_sensitive()),
                "value": float(scale.get_value()),
                "min": float(adj.get_lower()) if adj else 0.0,
                "max": float(adj.get_upper()) if adj else 100.0,
                "step": float(adj.get_step_increment()) if adj else 1.0,
                "digits": digits,
            }

        combo_box = find_child_by_type(row, Gtk.ComboBox)
        if combo_box is not None:
            opts = extract_model_strings(combo_box.get_model())
            sel = combo_box.get_active()
            if sel < 0:
                sel = 0
            return {
                "index": idx,
                "type": "ComboRow",
                "title": title,
                "subtitle": subtitle,
                "visible": visible,
                "sensitive": sensitive and bool(combo_box.get_sensitive()),
                "options": opts,
                "selected": int(sel),
            }

        dd = find_child_by_type(row, Gtk.DropDown)
        if dd is not None:
            opts = extract_model_strings(dd.get_model())
            sel = dd.get_selected()
            if sel == 4294967295 or sel < 0:
                sel = 0
            return {
                "index": idx,
                "type": "ComboRow",
                "title": title,
                "subtitle": subtitle,
                "visible": visible,
                "sensitive": sensitive and bool(dd.get_sensitive()),
                "options": opts,
                "selected": int(sel),
            }

        sw = find_child_by_type(row, Gtk.Switch)
        if sw is not None:
            return {
                "index": idx,
                "type": "SwitchRow",
                "title": title,
                "subtitle": subtitle,
                "visible": visible,
                "sensitive": sensitive and bool(sw.get_sensitive()),
                "active": bool(sw.get_active()),
            }

        spin = find_child_by_type(row, Gtk.SpinButton)
        if spin is not None:
            adj = spin.get_adjustment()
            return {
                "index": idx,
                "type": "SpinRow",
                "title": title,
                "subtitle": subtitle,
                "visible": visible,
                "sensitive": sensitive and bool(spin.get_sensitive()),
                "value": float(spin.get_value()),
                "min": float(adj.get_lower()) if adj else 0.0,
                "max": float(adj.get_upper()) if adj else 100.0,
                "step": float(adj.get_step_increment()) if adj else 1.0,
                "digits": int(spin.get_digits()),
            }

        pw_entry = find_child_by_type(row, Gtk.PasswordEntry)
        if pw_entry is not None:
            return {
                "index": idx,
                "type": "PasswordRow",
                "title": title,
                "subtitle": subtitle,
                "visible": visible,
                "sensitive": sensitive and bool(pw_entry.get_sensitive()),
                "editable": bool(pw_entry.get_editable()) if hasattr(pw_entry, "get_editable") else True,
                "text": pw_entry.get_text() or "",
            }

        entry = find_child_by_type(row, Gtk.Entry)
        if entry is not None:
            return {
                "index": idx,
                "type": "EntryRow",
                "title": title,
                "subtitle": subtitle,
                "visible": visible,
                "sensitive": sensitive and bool(entry.get_sensitive()),
                "editable": bool(entry.get_editable()) if hasattr(entry, "get_editable") else True,
                "text": entry.get_text() or "",
            }

        btn = find_child_by_type(row, Gtk.Button)
        if btn is not None:
            icon_name = btn.get_icon_name() if hasattr(btn, "get_icon_name") else ""
            label = btn.get_label() if hasattr(btn, "get_label") else ""
            suggested = btn.has_css_class("suggested-action") if hasattr(btn, "has_css_class") else False
            return {
                "index": idx,
                "type": "ButtonRow",
                "title": title,
                "subtitle": subtitle,
                "visible": visible,
                "sensitive": sensitive and bool(btn.get_sensitive()),
                "button_icon": icon_name or "",
                "button_label": label or "",
                "suggested": bool(suggested),
            }

        return {
            "index": idx,
            "type": "ActionRow",
            "title": title,
            "subtitle": subtitle,
            "visible": visible,
            "sensitive": sensitive,
        }

    # 8. Standalone GTK widgets from get_custom_config_area() or Gtk.Box settings
    if isinstance(row, Gtk.Button):
        icon_name = row.get_icon_name() if hasattr(row, "get_icon_name") else ""
        label = row.get_label() if hasattr(row, "get_label") else ""
        suggested = row.has_css_class("suggested-action") if hasattr(row, "has_css_class") else False
        return {
            "index": idx,
            "type": "ButtonRow",
            "title": label or "Action",
            "subtitle": row.get_tooltip_text() or "",
            "visible": visible,
            "sensitive": sensitive,
            "button_icon": icon_name or "",
            "button_label": label or "",
            "suggested": bool(suggested),
        }

    if isinstance(row, Gtk.Scale):
        adj = row.get_adjustment()
        return {
            "index": idx,
            "type": "ScaleRow",
            "title": row.get_tooltip_text() or "Value",
            "subtitle": "",
            "visible": visible,
            "sensitive": sensitive,
            "value": float(row.get_value()),
            "min": float(adj.get_lower()) if adj else 0.0,
            "max": float(adj.get_upper()) if adj else 100.0,
            "step": float(adj.get_step_increment()) if adj else 1.0,
            "digits": int(row.get_digits()),
        }

    if isinstance(row, (Gtk.Switch, Gtk.CheckButton)):
        lbl = row.get_label() if hasattr(row, "get_label") else ""
        return {
            "index": idx,
            "type": "SwitchRow",
            "title": lbl or "Enabled",
            "subtitle": "",
            "visible": visible,
            "sensitive": sensitive,
            "active": bool(row.get_active()),
        }

    if isinstance(row, Gtk.PasswordEntry):
        return {
            "index": idx,
            "type": "PasswordRow",
            "title": row.get_placeholder_text() or "Password",
            "subtitle": "",
            "visible": visible,
            "sensitive": sensitive,
            "editable": True,
            "text": row.get_text() or "",
        }

    if isinstance(row, Gtk.Entry):
        return {
            "index": idx,
            "type": "EntryRow",
            "title": row.get_placeholder_text() or "Text",
            "subtitle": "",
            "visible": visible,
            "sensitive": sensitive,
            "editable": bool(row.get_editable()),
            "text": row.get_text() or "",
        }

    return {
        "index": idx,
        "type": "ActionRow",
        "title": title or type(row).__name__,
        "subtitle": subtitle,
        "visible": visible,
        "sensitive": sensitive,
    }


def apply_row_mutation(row, req: dict) -> str:
    """Apply a UI change from Ballista onto the live GTK widget, firing its real Python signals."""
    if isinstance(row, Adw.ComboRow):
        if "selected" in req and req["selected"] is not None:
            sel = int(req["selected"])
            m = row.get_model()
            n = m.get_n_items() if (m and hasattr(m, "get_n_items")) else 0
            if n > 0 and 0 <= sel < n:
                row.set_selected(sel)
            return "ok"

    elif isinstance(row, Adw.SwitchRow):
        if "active" in req and req["active"] is not None:
            row.set_active(bool(req["active"]))
            return "ok"

    elif isinstance(row, Adw.ExpanderRow):
        if "active" in req and req["active"] is not None:
            row.set_enable_expansion(bool(req["active"]))
            return "ok"

    elif isinstance(row, (Adw.PasswordEntryRow, Adw.EntryRow)):
        if "text" in req and req["text"] is not None:
            new_text = str(req["text"])
            if row.get_text() != new_text:
                row.set_text(new_text)
            return "ok"

    elif isinstance(row, Adw.SpinRow):
        if "value" in req and req["value"] is not None:
            row.set_value(float(req["value"]))
            return "ok"

    elif isinstance(row, Adw.ActionRow):
        if req.get("trigger_button"):
            btn = find_child_by_type(row, Gtk.Button)
            if btn is not None:
                btn.emit("clicked")
                return "ok"

        color_btn = find_child_by_type(row, Gtk.ColorButton) or find_child_by_type(row, Gtk.ColorDialogButton)
        if color_btn is not None and req.get("color"):
            rgba = Gdk.RGBA()
            rgba.parse(str(req["color"]))
            color_btn.set_rgba(rgba)
            if isinstance(color_btn, Gtk.ColorButton):
                color_btn.emit("color-set")
            return "ok"

        scale = find_child_by_type(row, Gtk.Scale)
        if scale is not None and "value" in req and req["value"] is not None:
            scale.set_value(float(req["value"]))
            return "ok"

        combo_box = find_child_by_type(row, Gtk.ComboBox)
        if combo_box is not None and "selected" in req and req["selected"] is not None:
            combo_box.set_active(int(req["selected"]))
            return "ok"

        dd = find_child_by_type(row, Gtk.DropDown)
        if dd is not None and "selected" in req and req["selected"] is not None:
            dd.set_selected(int(req["selected"]))
            return "ok"

        sw = find_child_by_type(row, Gtk.Switch)
        if sw is not None and "active" in req and req["active"] is not None:
            sw.set_active(bool(req["active"]))
            return "ok"

        spin = find_child_by_type(row, Gtk.SpinButton)
        if spin is not None and "value" in req and req["value"] is not None:
            spin.set_value(float(req["value"]))
            return "ok"

        pw = find_child_by_type(row, Gtk.PasswordEntry)
        if pw is not None and "text" in req and req["text"] is not None:
            if pw.get_text() != str(req["text"]):
                pw.set_text(str(req["text"]))
            return "ok"

        entry = find_child_by_type(row, Gtk.Entry)
        if entry is not None and "text" in req and req["text"] is not None:
            if entry.get_text() != str(req["text"]):
                entry.set_text(str(req["text"]))
            return "ok"

    else:
        if isinstance(row, Gtk.Button) and req.get("trigger_button"):
            row.emit("clicked")
            return "ok"
        if isinstance(row, Gtk.Scale) and "value" in req and req["value"] is not None:
            row.set_value(float(req["value"]))
            return "ok"
        if isinstance(row, (Gtk.Switch, Gtk.CheckButton)) and "active" in req and req["active"] is not None:
            row.set_active(bool(req["active"]))
            return "ok"
        if isinstance(row, (Gtk.Entry, Gtk.PasswordEntry)) and "text" in req and req["text"] is not None:
            if row.get_text() != str(req["text"]):
                row.set_text(str(req["text"]))
            return "ok"

    return "ignored"


def attach_widget_listeners(rows: list, emit_fn, bound_ids: set = None):
    """Attach GTK change signals on every row widget so asynchronous plugin updates stream live to Ballista."""
    for row in rows:
        if bound_ids is not None:
            rid = id(row)
            if rid in bound_ids:
                continue
            bound_ids.add(rid)
        try:
            row.connect("notify::visible", lambda *_: emit_fn())
            row.connect("notify::sensitive", lambda *_: emit_fn())
            if hasattr(row, "get_subtitle"):
                row.connect("notify::subtitle", lambda *_: emit_fn())
            if hasattr(row, "get_title"):
                row.connect("notify::title", lambda *_: emit_fn())
        except Exception:
            pass

        if isinstance(row, Adw.ComboRow):
            row.connect("notify::selected", lambda *_: emit_fn())

            def _bind_model(m):
                if m is not None and hasattr(m, "connect"):
                    try:
                        m.connect("items-changed", lambda *_: emit_fn())
                    except Exception:
                        pass

            def _on_model_changed(r, *args):
                _bind_model(r.get_model())
                emit_fn()

            row.connect("notify::model", _on_model_changed)
            _bind_model(row.get_model())

        elif isinstance(row, Adw.SwitchRow):
            row.connect("notify::active", lambda *_: emit_fn())

        elif isinstance(row, Adw.ExpanderRow):
            row.connect("notify::enable-expansion", lambda *_: emit_fn())
            row.connect("notify::expanded", lambda *_: emit_fn())
            if hasattr(row, "_on_rows_changed_callbacks") and isinstance(row._on_rows_changed_callbacks, list):
                row._on_rows_changed_callbacks.append(emit_fn)

        elif isinstance(row, (Adw.PasswordEntryRow, Adw.EntryRow)):
            row.connect("changed", lambda *_: emit_fn())

        elif isinstance(row, Adw.SpinRow):
            row.connect("notify::value", lambda *_: emit_fn())

        elif isinstance(row, Adw.ActionRow):
            color_btn = find_child_by_type(row, Gtk.ColorButton) or find_child_by_type(row, Gtk.ColorDialogButton)
            if color_btn is not None:
                if isinstance(color_btn, Gtk.ColorButton):
                    color_btn.connect("color-set", lambda *_: emit_fn())
                else:
                    color_btn.connect("notify::rgba", lambda *_: emit_fn())

            scale = find_child_by_type(row, Gtk.Scale)
            if scale is not None:
                scale.connect("value-changed", lambda *_: emit_fn())

            combo_box = find_child_by_type(row, Gtk.ComboBox)
            if combo_box is not None:
                combo_box.connect("changed", lambda *_: emit_fn())

            dd = find_child_by_type(row, Gtk.DropDown)
            if dd is not None:
                dd.connect("notify::selected", lambda *_: emit_fn())
                dd.connect("notify::model", lambda *_: emit_fn())

            sw = find_child_by_type(row, Gtk.Switch)
            if sw is not None:
                sw.connect("notify::active", lambda *_: emit_fn())

            spin = find_child_by_type(row, Gtk.SpinButton)
            if spin is not None:
                spin.connect("value-changed", lambda *_: emit_fn())

            pw = find_child_by_type(row, Gtk.PasswordEntry)
            if pw is not None:
                pw.connect("changed", lambda *_: emit_fn())

            entry = find_child_by_type(row, Gtk.Entry)
            if entry is not None:
                entry.connect("changed", lambda *_: emit_fn())

            btn = find_child_by_type(row, Gtk.Button)
            if btn is not None:
                try:
                    btn.connect("notify::label", lambda *_: emit_fn())
                    btn.connect("notify::sensitive", lambda *_: emit_fn())
                except Exception:
                    pass
        elif isinstance(row, Gtk.Button):
            try:
                row.connect("notify::label", lambda *_: emit_fn())
                row.connect("notify::sensitive", lambda *_: emit_fn())
            except Exception:
                pass
        elif isinstance(row, Gtk.Scale):
            row.connect("value-changed", lambda *_: emit_fn())
        elif isinstance(row, (Gtk.Switch, Gtk.CheckButton)):
            row.connect("notify::active", lambda *_: emit_fn())
        elif isinstance(row, (Gtk.Entry, Gtk.PasswordEntry)):
            row.connect("changed", lambda *_: emit_fn())


class BridgeRunner:
    def __init__(self):
        self.plugins = {}                     # plugin_id -> PluginBase instance
        self.actions = {}                     # slot -> ActionCore/ActionBase instance
        self.action_root_widgets = {}         # slot -> root GTK widgets for dynamic re-flattening
        self.action_config_rows = {}          # slot -> flattened list of GTK widgets
        self.action_bound_ids = {}            # slot -> set of widget ids with attached listeners
        self.plugin_settings_root_widgets = {}
        self.plugin_settings_rows = {}        # plugin_id -> flattened list of GTK widgets
        self.plugin_settings_bound_ids = {}
        self._pending_emit_rows = set()
        self._pending_emit_plugin_rows = set()
        self._last_action_rows_hash = {}
        self._last_plugin_rows_hash = {}
        self._emit_lock = threading.Lock()
        self.loop = None

        self.deck_controller = DeckController(runner=self)
        self.page = Page(deck_controller=self.deck_controller)
        self.deck_controller.active_page = self.page

        # Wire StreamController globals proxies
        gl.app._runner = self
        gl.plugin_manager._runner = self
        gl.deck_manager.deck_controllers = [self.deck_controller]
        gl.page_manager.pages = [self.page]

    def emit(self, event_type: str, data: dict):
        payload = {"event": event_type}
        payload.update(data)
        try:
            line = json.dumps(payload, default=str)
        except Exception as e:
            logger.error(f"Failed to serialize bridge event {event_type}: {e}")
            return
        with self._emit_lock:
            sys.stdout.write(line + "\n")
            sys.stdout.flush()

    def emit_set_media(self, slot: str, media_path: str, size=None, valign=None, halign=None):
        data = {"slot": slot, "media_path": media_path}
        if size is not None:
            data["size"] = float(size)
        if valign is not None:
            data["valign"] = float(valign)
        if halign is not None:
            data["halign"] = float(halign)
        self.emit("set_media", data)

    def emit_set_label(
        self,
        slot: str,
        text: str,
        position: str = "bottom",
        color=None,
        font_family=None,
        font_size=None,
        outline_width=None,
        outline_color=None,
    ):
        data = {"slot": slot, "text": text, "position": position or "bottom"}
        if color is not None:
            data["color"] = color
        if font_family is not None:
            data["font_family"] = str(font_family)
        if font_size is not None:
            try:
                data["font_size"] = int(font_size)
            except Exception:
                pass
        if outline_width is not None:
            data["outline_width"] = outline_width
        if outline_color is not None:
            data["outline_color"] = outline_color
        self.emit("set_label", data)

    def emit_set_state(self, slot: str, state_index: int):
        self.emit("set_state", {"slot": slot, "state": int(state_index)})

    def emit_set_settings(self, slot: str, settings: dict):
        self.emit("set_settings", {"slot": slot, "settings": settings or {}})

    def emit_set_background_color(self, slot: str, color):
        self.emit("set_background_color", {"slot": slot, "color": color})

    def emit_action_config_rows(self, slot: str, force: bool = False):
        if slot in self._pending_emit_rows and not force:
            return

        def _do_emit():
            self._pending_emit_rows.discard(slot)
            if slot not in self.actions:
                return False
            roots = self.action_root_widgets.get(slot)
            if roots is not None:
                rows = flatten_config_widgets(roots)
                self.action_config_rows[slot] = rows
                bound = self.action_bound_ids.setdefault(slot, set())
                emit_cb = lambda s=slot: self.emit_action_config_rows(s)
                attach_widget_listeners(rows, emit_cb, bound_ids=bound)
                for rw in roots:
                    if isinstance(rw, Adw.ExpanderRow) and hasattr(rw, "_on_rows_changed_callbacks"):
                        eid = ("exp", id(rw))
                        if eid not in bound:
                            bound.add(eid)
                            rw._on_rows_changed_callbacks.append(emit_cb)
            else:
                rows = self.action_config_rows.get(slot, [])
            rows_data = [serialize_widget_row(r, idx) for idx, r in enumerate(rows)]
            sig = json.dumps(rows_data, sort_keys=True)
            if not force and self._last_action_rows_hash.get(slot) == sig:
                return False
            self._last_action_rows_hash[slot] = sig
            self.emit("action_config_rows", {"slot": slot, "rows": rows_data})
            return False

        self._pending_emit_rows.add(slot)
        GLib.idle_add(_do_emit)

    def emit_plugin_settings_rows(self, plugin_id: str, force: bool = False):
        if plugin_id in self._pending_emit_plugin_rows and not force:
            return

        def _do_emit():
            self._pending_emit_plugin_rows.discard(plugin_id)
            roots = self.plugin_settings_root_widgets.get(plugin_id)
            if roots is not None:
                rows = flatten_config_widgets(roots)
                self.plugin_settings_rows[plugin_id] = rows
                bound = self.plugin_settings_bound_ids.setdefault(plugin_id, set())
                emit_cb = lambda pid=plugin_id: self.emit_plugin_settings_rows(pid)
                attach_widget_listeners(rows, emit_cb, bound_ids=bound)
            else:
                rows = self.plugin_settings_rows.get(plugin_id, [])
            rows_data = [serialize_widget_row(r, idx) for idx, r in enumerate(rows)]
            sig = json.dumps(rows_data, sort_keys=True)
            if not force and self._last_plugin_rows_hash.get(plugin_id) == sig:
                return False
            self._last_plugin_rows_hash[plugin_id] = sig
            self.emit("plugin_settings_rows", {"plugin_id": plugin_id, "rows": rows_data})
            return False

        self._pending_emit_plugin_rows.add(plugin_id)
        GLib.idle_add(_do_emit)

    def _collect_plugin_metadata(self, plugin_id: str, plugin) -> dict:
        actions_meta = []
        seen_ids = set()

        holders = list(plugin.get_action_holders().values())
        for grp in getattr(plugin, "action_holder_groups", []):
            for h in getattr(grp, "action_holders", []):
                holders.append(h)

        for holder in holders:
            aid = getattr(holder, "action_id", "")
            if not aid or aid in seen_ids:
                continue
            seen_ids.add(aid)
            icon_path, icon_name = extract_gtk_image_path_or_name(getattr(holder, "icon", None))
            support = getattr(holder, "action_support", {}) or {}
            sup_key = support.get(Input.Key, ActionInputSupport.UNTESTED) != ActionInputSupport.UNSUPPORTED
            sup_dial = support.get(Input.Dial, ActionInputSupport.UNTESTED) != ActionInputSupport.UNSUPPORTED
            sup_touch = support.get(Input.Touchscreen, ActionInputSupport.UNTESTED) != ActionInputSupport.UNSUPPORTED
            actions_meta.append({
                "action_id": aid,
                "action_name": str(getattr(holder, "action_name", aid)),
                "icon_path": icon_path,
                "icon_name": icon_name,
                "supports_key": bool(sup_key),
                "supports_dial": bool(sup_dial),
                "supports_touchscreen": bool(sup_touch),
            })

        sel_icon_path, sel_icon_name = "", ""
        try:
            sel_widget = plugin.get_selector_icon()
            sel_icon_path, sel_icon_name = extract_gtk_image_path_or_name(sel_widget)
        except Exception:
            pass

        # Determine if plugin provides custom settings UI
        has_settings = bool(getattr(plugin, "has_plugin_settings", False))
        if not has_settings:
            # Check if subclass overrode get_settings_area or get_config_rows
            if type(plugin).get_settings_area is not PluginBase.get_settings_area:
                has_settings = True
            elif type(plugin).get_config_rows is not PluginBase.get_config_rows:
                has_settings = True

        return {
            "plugin_id": plugin_id,
            "plugin_name": str(getattr(plugin, "plugin_name", None) or plugin_id),
            "has_settings": has_settings,
            "selector_icon_path": sel_icon_path,
            "selector_icon_name": sel_icon_name,
            "actions": actions_meta,
        }

    def handle_init_plugin(self, plugin_id: str, plugin_path: str):
        if plugin_id in self.plugins:
            meta = self._collect_plugin_metadata(plugin_id, self.plugins[plugin_id])
            self.emit("plugin_initialized", meta)
            return {"status": "already_loaded", "plugin_id": plugin_id}

        plugin_path = os.path.abspath(plugin_path)
        parent_dir = os.path.dirname(plugin_path)
        grandparent_dir = os.path.dirname(parent_dir)
        folder_name = os.path.basename(plugin_path)

        if grandparent_dir and grandparent_dir not in sys.path:
            sys.path.insert(0, grandparent_dir)
        if parent_dir not in sys.path:
            sys.path.insert(0, parent_dir)
        # Keep the currently-loading plugin's path at index 0 for bare relative imports (e.g. `from actions.Foo import Foo`)
        if plugin_path in sys.path:
            sys.path.remove(plugin_path)
        sys.path.insert(0, plugin_path)
        # Clear any stale bare `actions` package from a previously loaded plugin so bare `from actions.X` resolves locally
        for stale_key in [k for k in list(sys.modules.keys()) if k == "actions" or k.startswith("actions.")]:
            sys.modules.pop(stale_key, None)

        # Ensure `plugins` namespace package resolves this plugin's parent directory
        if "plugins" not in sys.modules:
            import types
            pkg = types.ModuleType("plugins")
            pkg.__path__ = [parent_dir]
            sys.modules["plugins"] = pkg
        else:
            pkg = sys.modules["plugins"]
            if hasattr(pkg, "__path__") and parent_dir not in pkg.__path__:
                pkg.__path__.append(parent_dir)

        main_py = os.path.join(plugin_path, "main.py")
        if not os.path.exists(main_py):
            return {"status": "error", "message": f"main.py not found in {plugin_path}"}

        module = None
        last_err = None
        for mod_name in (f"plugins.{folder_name}.main", f"{folder_name}.main", f"{plugin_id}.main"):
            try:
                module = importlib.import_module(mod_name)
                break
            except Exception as e:
                last_err = e

        if module is None:
            try:
                spec = importlib.util.spec_from_file_location(f"{folder_name}.main", main_py)
                module = importlib.util.module_from_spec(spec)
                sys.modules[f"{folder_name}.main"] = module
                spec.loader.exec_module(module)
            except Exception as e:
                logger.error(f"Failed to import plugin {plugin_id} ({main_py}): {e}\n{traceback.format_exc()}")
                return {"status": "error", "message": f"Import failed for {plugin_id}: {e} (prev: {last_err})"}

        plugin_instance = None
        try:
            if hasattr(module, "PluginTemplate") and isinstance(module.PluginTemplate, type):
                plugin_instance = module.PluginTemplate()
            else:
                for attr in dir(module):
                    val = getattr(module, attr)
                    if isinstance(val, type) and issubclass(val, PluginBase) and val is not PluginBase:
                        plugin_instance = val()
                        break
        except Exception as e:
            logger.error(f"Failed to instantiate plugin {plugin_id}: {e}\n{traceback.format_exc()}")
            return {"status": "error", "message": f"Init error in {plugin_id}: {e}"}

        if plugin_instance is None:
            return {"status": "error", "message": f"No PluginBase subclass found in {main_py}"}

        self.plugins[plugin_id] = plugin_instance
        # Also register under manifest/folder id alias if different
        actual_id = plugin_instance.get_plugin_id()
        if actual_id and actual_id != plugin_id:
            self.plugins[actual_id] = plugin_instance

        logger.info(f"Initialized StreamController plugin {plugin_id} ({len(plugin_instance.get_action_holders())} actions)")
        meta = self._collect_plugin_metadata(plugin_id, plugin_instance)
        self.emit("plugin_initialized", meta)
        return {"status": "ok", "plugin_id": plugin_id}

    def handle_uninstall_plugin(self, plugin_id: str):
        # Destroy any actions belonging to this plugin
        to_remove = [
            slot for slot, act in self.actions.items()
            if getattr(getattr(act, "plugin_base", None), "plugin_id", None) == plugin_id
        ]
        for slot in to_remove:
            self.handle_destroy_action(slot)

        plugin = self.plugins.pop(plugin_id, None)
        self.plugin_settings_rows.pop(plugin_id, None)
        self._last_plugin_rows_hash.pop(plugin_id, None)
        if plugin is not None:
            try:
                plugin.on_close()
            except Exception:
                pass
            try:
                plugin.on_uninstall()
            except Exception:
                pass
            PluginBase.plugins.pop(plugin_id, None)
        return {"status": "ok"}

    def handle_create_action(self, slot: str, plugin_id: str, action_id: str, settings: dict):
        # Cleanly tear down any existing action in this slot first
        if slot in self.actions:
            self.handle_destroy_action(slot)

        plugin = self.plugins.get(plugin_id)
        if not plugin:
            # Try matching by plugin_id prefix from action_id
            if "::" in action_id:
                plugin = self.plugins.get(action_id.split("::")[0])
        if not plugin:
            return {"status": "error", "message": f"Plugin {plugin_id} not initialized"}

        action_class = None
        action_name = action_id

        # 1. Look up in plugin.action_holders (exact, suffix, or class name)
        holders = plugin.get_action_holders()
        if action_id in holders:
            action_class = holders[action_id].action_core
            action_name = holders[action_id].action_name
        else:
            req_stem = action_id.split("::")[-1]
            for h_id, holder in holders.items():
                h_stem = h_id.split("::")[-1]
                cls_name = getattr(holder.action_core, "__name__", "")
                if h_id.endswith(action_id) or action_id.endswith(h_id) or h_stem == req_stem or cls_name == req_stem:
                    action_class = holder.action_core
                    action_name = holder.action_name
                    action_id = h_id
                    break

        # 2. Dynamic file fallback
        if action_class is None:
            action_stem = action_id.split("::")[-1]
            act_file = os.path.join(plugin.PATH, "actions", action_stem, f"{action_stem}.py")
            if not os.path.exists(act_file):
                act_file = os.path.join(plugin.PATH, "actions", f"{action_stem}.py")
            if os.path.exists(act_file):
                spec = importlib.util.spec_from_file_location(f"act_{action_stem}", act_file)
                act_mod = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(act_mod)
                action_class = getattr(act_mod, action_stem, None)

        if action_class is None:
            return {"status": "error", "message": f"Could not find action class for {action_id}"}

        # Determine InputIdentifier
        if slot.startswith("key_"):
            input_ident = Input.Key(slot.replace("key_", ""))
        elif slot.startswith("dial_"):
            input_ident = Input.Dial(slot.replace("dial_", ""))
        else:
            input_ident = Input.Touchscreen("0")

        # Instantiate action
        try:
            action = action_class(
                action_id=action_id,
                action_name=action_name,
                deck_controller=self.deck_controller,
                page=self.page,
                plugin_base=plugin,
                state=0,
                input_ident=input_ident,
            )
        except Exception:
            action = action_class()
            action.action_id = action_id
            action.action_name = action_name
            action.deck_controller = self.deck_controller
            action.page = self.page
            action.plugin_base = plugin
            action.input_ident = input_ident

        action._ballista_runner = self
        action._ballista_slot = slot
        action._ballista_settings = dict(settings) if isinstance(settings, dict) else {}

        self.actions[slot] = action

        # Mark ready and call on_ready()
        action.on_ready_called = True
        try:
            action.on_ready()
        except Exception as e:
            logger.warning(f"Error in {action_id}.on_ready(): {e}\n{traceback.format_exc()}")

        # Collect ALL config UI widgets: GenerativeUI + get_config_rows() + get_custom_config_area()
        raw_widgets = []
        try:
            gen_widgets = action.get_generative_ui_widgets() or []
            if gen_widgets:
                action._do_load_initial_generative_ui()
        except Exception as e:
            logger.warning(f"Error loading generative UI for {slot}: {e}")
            gen_widgets = []

        try:
            cfg_rows = action.get_config_rows() or []
        except Exception as e:
            logger.warning(f"Error calling get_config_rows() for {slot}: {e}\n{traceback.format_exc()}")
            cfg_rows = []

        try:
            custom_area = action.get_custom_config_area()
        except Exception as e:
            logger.warning(f"Error calling get_custom_config_area() for {slot}: {e}")
            custom_area = None

        if getattr(action, "put_custom_config_rows_below_gen_ui", False):
            raw_widgets.extend(gen_widgets)
            raw_widgets.extend(cfg_rows)
        else:
            raw_widgets.extend(cfg_rows)
            raw_widgets.extend(gen_widgets)

        if custom_area is not None:
            raw_widgets.append(custom_area)

        self.action_root_widgets[slot] = list(raw_widgets)
        rows = flatten_config_widgets(raw_widgets)
        self.action_config_rows[slot] = rows
        bound = set()
        self.action_bound_ids[slot] = bound
        self._last_action_rows_hash.pop(slot, None)
        emit_cb = lambda s=slot: self.emit_action_config_rows(s)
        attach_widget_listeners(rows, emit_cb, bound_ids=bound)
        for rw in raw_widgets:
            if isinstance(rw, Adw.ExpanderRow) and hasattr(rw, "_on_rows_changed_callbacks"):
                eid = ("exp", id(rw))
                if eid not in bound:
                    bound.add(eid)
                    rw._on_rows_changed_callbacks.append(emit_cb)
        self.emit_action_config_rows(slot, force=True)

        logger.info(f"Created action instance for slot {slot}: {action_id} ({len(rows)} live config rows)")
        return {"status": "ok", "slot": slot}

    def handle_destroy_action(self, slot: str):
        old_act = self.actions.pop(slot, None)
        self.action_root_widgets.pop(slot, None)
        self.action_config_rows.pop(slot, None)
        self.action_bound_ids.pop(slot, None)
        self._last_action_rows_hash.pop(slot, None)
        if old_act is not None:
            try:
                old_act.on_removed_from_cache()
            except Exception:
                pass
        return {"status": "ok"}

    def handle_clear_all_actions(self):
        for slot in list(self.actions.keys()):
            self.handle_destroy_action(slot)
        return {"status": "ok"}

    def handle_update_action_settings(self, slot: str, settings: dict):
        action = self.actions.get(slot)
        if action:
            action._ballista_settings = dict(settings) if isinstance(settings, dict) else {}
            try:
                action.on_ready()
            except Exception as e:
                logger.warning(f"Error in on_ready() during settings update for {slot}: {e}")
            self.emit_action_config_rows(slot)
            return {"status": "ok"}
        return {"status": "not_found"}

    def handle_get_action_config_rows(self, slot: str):
        if slot in self.action_config_rows:
            self.emit_action_config_rows(slot, force=True)
            return {"status": "ok", "slot": slot}
        return {"status": "not_found"}

    def handle_set_action_config_row(self, slot: str, index: int, req: dict):
        rows = self.action_config_rows.get(slot, [])
        if index < 0 or index >= len(rows):
            return {"status": "error", "message": f"Index {index} out of range (total: {len(rows)})"}
        try:
            status = apply_row_mutation(rows[index], req)
            self.emit_action_config_rows(slot)
            return {"status": status}
        except Exception as e:
            logger.error(f"Error setting action config row {slot}[{index}]: {e}\n{traceback.format_exc()}")
            return {"status": "error", "message": str(e)}

    def handle_trigger_action_config_button(self, slot: str, index: int):
        rows = self.action_config_rows.get(slot, [])
        if index < 0 or index >= len(rows):
            return {"status": "error", "message": f"Index {index} out of range"}
        try:
            status = apply_row_mutation(rows[index], {"trigger_button": True})
            self.emit_action_config_rows(slot)
            return {"status": status}
        except Exception as e:
            logger.error(f"Error triggering button {slot}[{index}]: {e}")
            return {"status": "error", "message": str(e)}

    # ---------- Plugin Global Settings Bridge ---------- #
    def handle_get_plugin_settings_rows(self, plugin_id: str):
        plugin = self.plugins.get(plugin_id)
        if not plugin:
            self.emit("plugin_settings_rows", {"plugin_id": plugin_id, "rows": []})
            return {"status": "not_found"}

        raw_widgets = []
        try:
            area = plugin.get_settings_area()
            if area is not None:
                raw_widgets.append(area)
        except Exception as e:
            logger.warning(f"Error calling {plugin_id}.get_settings_area(): {e}\n{traceback.format_exc()}")

        try:
            cfg_rows = plugin.get_config_rows() or []
            raw_widgets.extend(cfg_rows)
        except Exception as e:
            logger.warning(f"Error calling {plugin_id}.get_config_rows(): {e}")

        self.plugin_settings_root_widgets[plugin_id] = list(raw_widgets)
        rows = flatten_config_widgets(raw_widgets)
        self.plugin_settings_rows[plugin_id] = rows
        bound = set()
        self.plugin_settings_bound_ids[plugin_id] = bound
        self._last_plugin_rows_hash.pop(plugin_id, None)
        attach_widget_listeners(rows, lambda pid=plugin_id: self.emit_plugin_settings_rows(pid), bound_ids=bound)
        self.emit_plugin_settings_rows(plugin_id, force=True)
        return {"status": "ok", "count": len(rows)}

    def handle_set_plugin_settings_row(self, plugin_id: str, index: int, req: dict):
        rows = self.plugin_settings_rows.get(plugin_id, [])
        if index < 0 or index >= len(rows):
            return {"status": "error", "message": f"Plugin settings index {index} out of range"}
        try:
            status = apply_row_mutation(rows[index], req)
            self.emit_plugin_settings_rows(plugin_id)
            return {"status": status}
        except Exception as e:
            logger.error(f"Error setting plugin settings row {plugin_id}[{index}]: {e}\n{traceback.format_exc()}")
            return {"status": "error", "message": str(e)}

    def handle_trigger_plugin_settings_button(self, plugin_id: str, index: int):
        rows = self.plugin_settings_rows.get(plugin_id, [])
        if index < 0 or index >= len(rows):
            return {"status": "error", "message": f"Plugin settings index {index} out of range"}
        try:
            status = apply_row_mutation(rows[index], {"trigger_button": True})
            self.emit_plugin_settings_rows(plugin_id)
            return {"status": status}
        except Exception as e:
            logger.error(f"Error triggering plugin settings button {plugin_id}[{index}]: {e}")
            return {"status": "error", "message": str(e)}

    # ---------- Hardware Input Event Dispatch ---------- #
    def handle_key_event(self, slot: str, event_type: str):
        action = self.actions.get(slot)
        if not action:
            return {"status": "no_action"}

        evt_map = {
            "down": Input.Key.Events.DOWN,
            "up": Input.Key.Events.UP,
            "short_up": Input.Key.Events.SHORT_UP,
            "hold_start": Input.Key.Events.HOLD_START,
            "hold_stop": Input.Key.Events.HOLD_STOP,
        }
        evt = evt_map.get(event_type.lower(), Input.Key.Events.DOWN)
        try:
            action._raw_event_callback(evt, None)
            if evt == Input.Key.Events.UP:
                # Also dispatch SHORT_UP for actions that bind SHORT_UP assigners
                assigner = action.event_manager.get_event_assigner_for_event(Input.Key.Events.SHORT_UP)
                if assigner is not None and assigner.id != "Key Short Up":
                    action._raw_event_callback(Input.Key.Events.SHORT_UP, None)
            return {"status": "ok"}
        except Exception as e:
            logger.error(f"Error handling key event for {slot}: {e}\n{traceback.format_exc()}")
            return {"status": "error", "message": str(e)}

    def handle_dial_event(self, slot: str, event_type: str, delta: int = 0):
        action = self.actions.get(slot)
        if not action:
            return {"status": "no_action"}

        try:
            if event_type == "turn":
                evt = Input.Dial.Events.TURN_CW if delta > 0 else Input.Dial.Events.TURN_CCW
                action._raw_event_callback(evt, {"delta": delta, "ticks": abs(delta)})
            elif event_type == "press":
                action._raw_event_callback(Input.Dial.Events.DOWN, None)
            elif event_type == "release":
                action._raw_event_callback(Input.Dial.Events.UP, None)
            elif event_type == "tap":
                action._raw_event_callback(Input.Dial.Events.SHORT_TOUCH_PRESS, None)
            elif event_type == "long_tap":
                action._raw_event_callback(Input.Dial.Events.LONG_TOUCH_PRESS, None)
            return {"status": "ok"}
        except Exception as e:
            logger.error(f"Error handling dial event for {slot}: {e}\n{traceback.format_exc()}")
            return {"status": "error", "message": str(e)}

    def handle_shutdown(self):
        gl.threads_running = False
        self.handle_clear_all_actions()
        for pid, plugin in list(self.plugins.items()):
            try:
                plugin.on_close()
            except Exception:
                pass
            try:
                plugin.on_disconnect()
            except Exception:
                pass
        if self.loop is not None:
            GLib.idle_add(self.loop.quit)
        return {"status": "ok"}

    def tick_actions(self) -> bool:
        if not gl.threads_running:
            return False
        for slot, action in list(self.actions.items()):
            if not getattr(action, "on_ready_called", False):
                continue
            # Only call on_tick if subclass actually implements it (ActionCore may inherit rpyc.Service)
            if getattr(type(action), "on_tick", None) is not ActionCore.on_tick:
                try:
                    action.on_tick()
                except Exception as e:
                    logger.debug(f"Error in on_tick() for {slot}: {e}")
        return True

    def process_command(self, cmd_line: str):
        if not cmd_line.strip():
            return
        try:
            req = json.loads(cmd_line)
            cmd = req.get("cmd")
            res = {}

            if cmd == "init_plugin":
                res = self.handle_init_plugin(req.get("plugin_id", ""), req.get("plugin_path", ""))
            elif cmd == "uninstall_plugin":
                res = self.handle_uninstall_plugin(req.get("plugin_id", ""))
            elif cmd == "create_action":
                res = self.handle_create_action(
                    req.get("slot", ""),
                    req.get("plugin_id", ""),
                    req.get("action_id", ""),
                    req.get("settings", {}),
                )
            elif cmd == "destroy_action":
                res = self.handle_destroy_action(req.get("slot", ""))
            elif cmd == "clear_all_actions":
                res = self.handle_clear_all_actions()
            elif cmd == "update_action_settings":
                res = self.handle_update_action_settings(req.get("slot", ""), req.get("settings", {}))
            elif cmd == "get_action_config_rows":
                res = self.handle_get_action_config_rows(req.get("slot", ""))
            elif cmd == "set_action_config_row":
                res = self.handle_set_action_config_row(req.get("slot", ""), int(req.get("index", 0)), req)
            elif cmd == "trigger_action_config_button":
                res = self.handle_trigger_action_config_button(req.get("slot", ""), int(req.get("index", 0)))
            elif cmd == "get_plugin_settings_rows":
                res = self.handle_get_plugin_settings_rows(req.get("plugin_id", ""))
            elif cmd == "set_plugin_settings_row":
                res = self.handle_set_plugin_settings_row(req.get("plugin_id", ""), int(req.get("index", 0)), req)
            elif cmd == "trigger_plugin_settings_button":
                res = self.handle_trigger_plugin_settings_button(req.get("plugin_id", ""), int(req.get("index", 0)))
            elif cmd == "key_event":
                res = self.handle_key_event(req.get("slot", ""), req.get("event", "down"))
            elif cmd == "dial_event":
                res = self.handle_dial_event(req.get("slot", ""), req.get("event", "turn"), int(req.get("delta", 0)))
            elif cmd == "shutdown":
                res = self.handle_shutdown()
            else:
                res = {"status": "unknown_cmd", "cmd": cmd}

            if "req_id" in req:
                res["req_id"] = req["req_id"]
            self.emit("cmd_result", res)

        except Exception as e:
            logger.error(f"Error processing command '{cmd_line}': {e}\n{traceback.format_exc()}")
            self.emit("cmd_result", {"status": "exception", "error": str(e)})


def main():
    runner = BridgeRunner()
    loop = GLib.MainLoop()
    runner.loop = loop
    runner.emit("ready", {"version": "2.0.0"})

    # Official StreamController 1-second on_tick() heartbeat
    GLib.timeout_add_seconds(1, runner.tick_actions)

    def stdin_reader():
        try:
            for line in sys.stdin:
                line_str = line.strip()
                if not line_str:
                    continue
                GLib.idle_add(runner.process_command, line_str)
        except Exception as e:
            logger.error(f"Bridge stdin read error: {e}")
        finally:
            GLib.idle_add(runner.handle_shutdown)

    reader_thread = threading.Thread(target=stdin_reader, daemon=True)
    reader_thread.start()

    try:
        loop.run()
    except (KeyboardInterrupt, SystemExit):
        runner.handle_shutdown()


if __name__ == "__main__":
    main()
