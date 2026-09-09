import assert from "node:assert/strict";
import test from "node:test";
import { defaultGeneralSettings } from "../src/api/generalModel.ts";
import {
  applyTheme,
  normalizeGeneralSettings,
} from "../src/features/settings/generalSettings.ts";

test("normalizes missing and invalid general settings to defaults", () => {
  assert.deepEqual(normalizeGeneralSettings({}), {
    theme: "dark",
    restore_tabs: true,
    recent_searches_limit: 10,
    csv_delimiter: ",",
    photos_taxon_name_parts: {
      sci_name: true,
      zh_name: true,
      en_name: true,
    },
    taxonomy_taxon_name_parts: {
      sci_name: true,
      zh_name: true,
      en_name: true,
    },
  });
  assert.deepEqual(normalizeGeneralSettings({
    theme: "invalid" as never,
    restore_tabs: "yes" as never,
    recent_searches_limit: 51,
  }), defaultGeneralSettings());
});

test("preserves valid general settings", () => {
  assert.deepEqual(normalizeGeneralSettings({
    theme: "light",
    restore_tabs: false,
    recent_searches_limit: 1,
    csv_delimiter: "\t",
    photos_taxon_name_parts: {
      sci_name: false,
      zh_name: true,
      en_name: false,
    },
    taxonomy_taxon_name_parts: {
      sci_name: true,
      zh_name: false,
      en_name: true,
    },
  }), {
    theme: "light",
    restore_tabs: false,
    recent_searches_limit: 1,
    csv_delimiter: "\t",
    photos_taxon_name_parts: {
      sci_name: false,
      zh_name: true,
      en_name: false,
    },
    taxonomy_taxon_name_parts: {
      sci_name: true,
      zh_name: false,
      en_name: true,
    },
  });
});

test("keeps at least one name part visible in each independent context", () => {
  assert.deepEqual(normalizeGeneralSettings({
    photos_taxon_name_parts: {
      sci_name: false,
      zh_name: false,
      en_name: false,
    },
  }), defaultGeneralSettings());

  const photosOnly = normalizeGeneralSettings({
    photos_taxon_name_parts: {
      sci_name: false,
      zh_name: true,
      en_name: false,
    },
  });
  assert.deepEqual(photosOnly.photos_taxon_name_parts, {
    sci_name: false,
    zh_name: true,
    en_name: false,
  });
  assert.deepEqual(photosOnly.taxonomy_taxon_name_parts, {
    sci_name: true,
    zh_name: true,
    en_name: true,
  });
});

test("applies forced themes and removes the override for system theme", () => {
  const root = { dataset: {} } as Pick<HTMLElement, "dataset">;
  applyTheme("dark", root);
  assert.equal(root.dataset.theme, "dark");
  applyTheme("light", root);
  assert.equal(root.dataset.theme, "light");
  applyTheme("system", root);
  assert.equal(root.dataset.theme, undefined);
});
