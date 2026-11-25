import os
import csv
import locale
import html
from loguru import logger


class LocaleManager:
    def __init__(self, csv_path: str = "") -> None:
        self.csv_path = csv_path or ""
        self.language = "en_US"
        self.FALLBACK_LOCALE = "en_US"
        self.available_locales: list[str] = []
        self.locale_data: dict = {}
        if self.csv_path:
            self.load_csv()

    def load_csv(self) -> None:
        if not self.csv_path or not os.path.exists(self.csv_path):
            return
        try:
            with open(self.csv_path, newline="", encoding="utf-8") as csvfile:
                reader = csv.reader(csvfile, delimiter=";", quotechar='"', skipinitialspace=True)
                header = next(reader, None)
                if not header:
                    return
                self.available_locales = header[1:]
                for row in reader:
                    if not row:
                        continue
                    translations = [value.replace("\\n", "\n") for value in row[1:]]
                    self.locale_data[row[0]] = dict(zip(self.available_locales, translations))
        except Exception as e:
            logger.warning(f"Error loading CSV locale {self.csv_path}: {e}")

    def set_language(self, language: str) -> str:
        self.language = self.get_best_match(language or self.FALLBACK_LOCALE)
        return self.language

    def set_fallback_language(self, language: str) -> None:
        self.FALLBACK_LOCALE = language

    def set_to_os_default(self):
        try:
            os_locale = locale.getlocale()[0]
        except Exception:
            os_locale = None
        self.set_language(self.FALLBACK_LOCALE if not os_locale else os_locale)

    def get_best_match(self, preferred_language: str) -> str:
        if preferred_language in self.available_locales:
            return preferred_language
        primary = preferred_language.split("_")[0]
        for lang in self.available_locales:
            if lang.startswith(primary):
                return lang
        return self.FALLBACK_LOCALE

    def get_custom_translation(self, locale_json: dict):
        if locale_json is None:
            return ""
        result = locale_json.get(self.language)
        if result in [None, ""]:
            return locale_json.get(self.FALLBACK_LOCALE, "")
        return result

    def get(self, key: str, fallback: str = None) -> str:
        key_dict = self.locale_data.get(key, {})
        if fallback is None:
            fallback = key
        result = key_dict.get(self.language)
        if result in [None, ""]:
            result = key_dict.get(self.FALLBACK_LOCALE, fallback)
        if result is None:
            result = fallback
        if result:
            result = html.escape(str(result))
        return result
