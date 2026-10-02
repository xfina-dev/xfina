<script setup>
import { computed, ref, watch } from 'vue';
import { seriesCsv, seriesCoverage } from 'xfina-wasm';
import { Download, FileJson, CheckCircle2, AlertTriangle, XCircle, ExternalLink } from 'lucide-vue-next';

import { Button } from '@/components/ui/button';
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from '@/components/ui/card';
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from '@/components/ui/table';
import { formatDate, formatDateTime, formatIsoDate, columnLabel, rateUnit } from '@/lib/format.js';

/**
 * Public data: what publishers hand to anyone, as opposed to anybody's
 * statement. Price, NAV and index histories, and published rate sheets.
 *
 * Files are grouped under the dataset they belong to -- one publisher's
 * series for one instrument -- because a long history arrives in pieces:
 * a year of NSE index levels per download, five years of AMFI NAVs. Each
 * piece is shown as what it is. Nothing here stitches them together; that is
 * computation over the data, and belongs to whoever uses it.
 */
const props = defineProps({
    // Parsed files whose area is "public".
    entries: { type: Array, required: true },
    // The format list from the library, for the publishers' download pages.
    formats: { type: Array, default: () => [] },
});

const HEADLINE_LABELS = {
    close: 'Close',
    nav: 'NAV',
    totalReturn: 'Total return',
    netTotalReturn: 'Net total return',
};
const FIELD_LABELS = {
    close: 'Close', high: 'High', low: 'Low', open: 'Open', volume: 'Volume',
    adjClose: 'Adj close', adjHigh: 'Adj high', adjLow: 'Adj low', adjOpen: 'Adj open', adjVolume: 'Adj volume',
    dividend: 'Dividend', splitFactor: 'Split', nav: 'NAV', adjNav: 'Adj NAV',
    totalReturn: 'Total return', netTotalReturn: 'Net total return',
};
const KIND_LABELS = {
    index: 'Index', etf: 'ETF', mutual_fund: 'Mutual fund',
    listed_security: 'Listed security', commodity_spot: 'Commodity spot',
};

const isSeries = (entry) => entry.response?.category === 'market_data';

// Coverage is the library's own reading of each file, computed once.
const coverageCache = new WeakMap();
const coverageOf = (entry) => {
    const data = entry.response?.data;
    if (!data) return null;
    if (!coverageCache.has(data)) coverageCache.set(data, seriesCoverage(data));
    return coverageCache.get(data);
};

/** A dataset is one publisher's series for one instrument. */
const datasetOf = (entry) => {
    const r = entry.response;
    if (!isSeries(entry)) return { key: r.format, title: 'Forex card rates', sub: r.institution };
    const d = r.data;
    const title = d.code || d.name || entry.name;
    return {
        key: `${r.format}|${title}`,
        title,
        // A name that only repeats the code or the publisher says nothing new.
        sub: d.name && d.name !== title && d.name !== r.institution ? d.name : null,
    };
};

const datasets = computed(() => {
    const groups = new Map();
    for (const entry of props.entries) {
        const { key, title, sub } = datasetOf(entry);
        if (!groups.has(key)) {
            groups.set(key, { key, title, sub, institution: entry.response.institution, format: entry.response.format, entries: [] });
        }
        groups.get(key).entries.push(entry);
    }
    return [...groups.values()]
        .map(group => {
            const first = group.entries[0];
            const pieces = group.entries
                .map(e => ({ entry: e, coverage: isSeries(e) ? coverageOf(e) : null }))
                // Oldest piece first, so a run of yearly downloads reads in order.
                .sort((a, b) => (a.coverage?.first || sheetDate(a.entry)).localeCompare(b.coverage?.first || sheetDate(b.entry)));
            const covered = pieces.map(p => p.coverage).filter(c => c?.first);
            return {
                ...group,
                series: isSeries(first) ? first.response.data : null,
                pieces,
                earliest: covered.map(c => c.first).sort()[0] || null,
                latest: covered.map(c => c.last).sort().slice(-1)[0] || null,
            };
        })
        .sort((a, b) => a.institution.localeCompare(b.institution) || a.title.localeCompare(b.title));
});

/** A rate sheet's date as an ISO string, for ordering it among the others. */
const sheetDate = (entry) => {
    const ts = entry.response?.data?.date;
    return ts ? new Date(Number(ts) * 1000).toISOString().slice(0, 10) : '';
};

const selectedId = ref(null);
const selected = computed(() =>
    props.entries.find(e => e.id === selectedId.value) ?? props.entries[0] ?? null
);
// A newly dropped pile settles on its first file rather than on nothing.
watch(() => props.entries.length, (n) => {
    if (n && !props.entries.some(e => e.id === selectedId.value)) selectedId.value = props.entries[0].id;
});

const selectedSeries = computed(() => (selected.value && isSeries(selected.value) ? selected.value.response.data : null));
const selectedSheet = computed(() => (selected.value && !isSeries(selected.value) ? selected.value.response.data : null));

/** The newest rows, newest first: what someone checking a download looks at. */
const PREVIEW_ROWS = 15;
const previewRows = computed(() => (selectedSeries.value?.rows || []).slice(-PREVIEW_ROWS).reverse());
const hasTime = computed(() => previewRows.value.some(r => r.time));

const statusIcon = (overall) => ({ passed: CheckCircle2, warning: AlertTriangle, failed: XCircle }[overall] || null);
const statusClass = (overall) => ({
    passed: 'text-emerald-500',
    warning: 'text-amber-500',
    failed: 'text-destructive',
}[overall] || 'text-muted-foreground');

/** The checks a file did not pass, by name, for the row's tooltip. */
const failedChecks = (entry) => {
    const summary = entry.response?.validation?.summary_level;
    return [...(summary?.declared?.checks || []), ...(summary?.derived?.checks || [])]
        .filter(c => !c.passed)
        .map(c => c.name.replaceAll('_', ' '));
};

const formatValue = (v) =>
    v === undefined || v === null ? '' : Number(v).toLocaleString(undefined, { maximumFractionDigits: 6 });

const stem = (name) => name.replace(/\.[^.]+$/, '');

const save = (content, filename, type) => {
    const url = URL.createObjectURL(new Blob([content], { type }));
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    a.click();
    URL.revokeObjectURL(url);
};

const downloadCsv = (entry) =>
    save(seriesCsv(entry.response.data), `${stem(entry.name)}.xfina.csv`, 'text/csv');
const downloadJson = (entry) =>
    save(JSON.stringify(entry.response.data, null, 2), `${stem(entry.name)}.xfina.json`, 'application/json');
/** Every public file read, as one JSON array of what each one parsed to. */
const downloadAll = () =>
    save(
        JSON.stringify(props.entries.map(e => ({ file: e.name, format: e.response.format, data: e.response.data })), null, 2),
        'xfina-public-data.json',
        'application/json',
    );

const publicFormats = computed(() => props.formats.filter(f => f.area === 'public'));
</script>

<template>
  <div class="space-y-6">
    <!-- Nothing dropped yet: say what belongs here, and where to get it. -->
    <Card v-if="!entries.length" class="bg-card border-border shadow-sm">
      <CardHeader>
        <CardTitle class="text-base">No public data yet</CardTitle>
        <CardDescription>
          Drop price, NAV or index history downloaded from any of these publishers. Each file is read on its own and shown under the dataset it belongs to.
        </CardDescription>
      </CardHeader>
      <CardContent class="flex flex-wrap gap-2">
        <a
          v-for="format in publicFormats"
          :key="format.id"
          :href="format.download_url"
          target="_blank"
          rel="noopener noreferrer"
          class="inline-flex items-center gap-1 rounded-md border border-border px-2 py-1 text-xs hover:bg-muted"
        >
          {{ format.institution }}
          <ExternalLink class="h-3 w-3 text-muted-foreground" />
        </a>
      </CardContent>
    </Card>

    <template v-else>
      <div class="flex items-center justify-between">
        <p class="text-sm text-muted-foreground">
          {{ datasets.length }} dataset{{ datasets.length === 1 ? '' : 's' }} from {{ entries.length }} file{{ entries.length === 1 ? '' : 's' }}.
          Pieces of one dataset are listed, not joined.
        </p>
        <Button variant="outline" size="sm" class="gap-2" @click="downloadAll">
          <FileJson class="h-4 w-4" /> All as JSON
        </Button>
      </div>

      <!-- One card per dataset, with a row per file. -->
      <Card v-for="dataset in datasets" :key="dataset.key" class="bg-card border-border shadow-sm">
        <CardHeader class="pb-3">
          <div class="flex flex-wrap items-baseline justify-between gap-2">
            <div class="min-w-0">
              <CardTitle class="truncate text-base" :title="dataset.title">{{ dataset.title }}</CardTitle>
              <CardDescription class="mt-0.5">
                {{ dataset.institution }}<span v-if="dataset.sub"> &middot; {{ dataset.sub }}</span>
              </CardDescription>
            </div>
            <div v-if="dataset.series" class="flex flex-wrap items-center gap-1.5 text-[11px]">
              <span class="rounded bg-primary/10 px-1.5 py-0.5 font-semibold text-primary">{{ HEADLINE_LABELS[dataset.series.headline] || dataset.series.headline }}</span>
              <span v-if="dataset.series.kind" class="rounded bg-muted px-1.5 py-0.5 text-muted-foreground">{{ KIND_LABELS[dataset.series.kind] }}</span>
              <span v-if="dataset.series.currency" class="rounded bg-muted px-1.5 py-0.5 font-mono text-muted-foreground">{{ dataset.series.currency }}</span>
              <span v-if="dataset.series.unit" class="rounded bg-muted px-1.5 py-0.5 text-muted-foreground">per {{ dataset.series.unit }}</span>
              <span class="rounded bg-muted px-1.5 py-0.5 text-muted-foreground">{{ dataset.series.frequency }}</span>
              <span v-if="dataset.earliest" class="tabular-nums text-muted-foreground">
                {{ formatIsoDate(dataset.earliest) }} – {{ formatIsoDate(dataset.latest) }}
              </span>
            </div>
          </div>
        </CardHeader>
        <CardContent class="overflow-x-auto pt-0">
          <Table>
            <TableHeader>
              <TableRow class="hover:bg-transparent">
                <TableHead class="text-muted-foreground">File</TableHead>
                <TableHead class="whitespace-nowrap text-muted-foreground">{{ dataset.series ? 'First' : 'Date' }}</TableHead>
                <TableHead v-if="dataset.series" class="whitespace-nowrap text-muted-foreground">Last</TableHead>
                <TableHead class="text-right text-muted-foreground">{{ dataset.series ? 'Rows' : 'Currencies' }}</TableHead>
                <TableHead v-if="dataset.series" class="text-right text-muted-foreground">Gaps</TableHead>
                <TableHead class="text-muted-foreground">Checks</TableHead>
                <TableHead class="text-right text-muted-foreground">Download</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              <TableRow
                v-for="{ entry, coverage } in dataset.pieces"
                :key="entry.id"
                class="cursor-pointer"
                :class="selected?.id === entry.id ? 'bg-primary/5' : ''"
                @click="selectedId = entry.id"
              >
                <TableCell class="max-w-[18rem] truncate font-mono text-xs" :title="entry.name">{{ entry.name }}</TableCell>
                <template v-if="coverage">
                  <TableCell class="whitespace-nowrap tabular-nums">{{ formatIsoDate(coverage.first) }}</TableCell>
                  <TableCell class="whitespace-nowrap tabular-nums">{{ formatIsoDate(coverage.last) }}</TableCell>
                  <TableCell class="text-right tabular-nums">{{ coverage.rows.toLocaleString() }}</TableCell>
                  <TableCell
                    class="text-right tabular-nums"
                    :class="coverage.gaps.length ? 'text-amber-500' : 'text-muted-foreground'"
                    :title="coverage.gaps.map(g => `${g.days} days after ${g.after}`).join('\n')"
                  >{{ coverage.gaps.length }}</TableCell>
                </template>
                <template v-else>
                  <TableCell class="whitespace-nowrap tabular-nums">{{ formatDate(entry.response.data.date) }}</TableCell>
                  <TableCell class="text-right tabular-nums">{{ entry.response.data.currencies?.length || 0 }}</TableCell>
                </template>
                <TableCell>
                  <span
                    class="flex items-center gap-1 text-xs font-medium"
                    :class="statusClass(entry.response.validation?.overall)"
                    :title="failedChecks(entry).join('\n') || 'Every check passed'"
                  >
                    <component :is="statusIcon(entry.response.validation?.overall)" class="h-3.5 w-3.5" />
                    {{ entry.response.validation?.overall }}
                  </span>
                </TableCell>
                <TableCell class="whitespace-nowrap text-right" @click.stop>
                  <Button v-if="coverage" variant="ghost" size="sm" class="h-7 gap-1 px-2" title="CSV, in Tiingo's column layout" @click="downloadCsv(entry)">
                    <Download class="h-3.5 w-3.5" /> CSV
                  </Button>
                  <Button variant="ghost" size="sm" class="h-7 gap-1 px-2" title="Everything the file parsed to" @click="downloadJson(entry)">
                    <FileJson class="h-3.5 w-3.5" /> JSON
                  </Button>
                </TableCell>
              </TableRow>
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      <hr class="border-border" />

      <!-- The selected file's newest rows. -->
      <Card v-if="selectedSeries" class="bg-card border-border shadow-sm">
        <CardHeader class="pb-2">
          <CardTitle class="text-sm font-semibold uppercase tracking-wider text-muted-foreground">
            Newest rows &middot; <span class="font-mono normal-case">{{ selected.name }}</span>
          </CardTitle>
          <CardDescription>
            Every field the file printed. A blank is a value the publisher did not print, never a zero.
            <span v-if="selectedSeries.extraFields?.length">Also in the JSON: {{ selectedSeries.extraFields.join(', ') }}.</span>
          </CardDescription>
        </CardHeader>
        <CardContent class="overflow-x-auto">
          <Table>
            <TableHeader>
              <TableRow class="hover:bg-transparent">
                <TableHead class="whitespace-nowrap text-muted-foreground">Date</TableHead>
                <TableHead v-if="hasTime" class="text-muted-foreground">Time</TableHead>
                <TableHead
                  v-for="field in selectedSeries.fields"
                  :key="field"
                  class="whitespace-nowrap text-right"
                  :class="field === selectedSeries.headline ? 'text-foreground' : 'text-muted-foreground'"
                >{{ FIELD_LABELS[field] || field }}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              <TableRow v-for="(row, idx) in previewRows" :key="idx">
                <TableCell class="whitespace-nowrap tabular-nums">{{ formatDate(row.date) }}</TableCell>
                <TableCell v-if="hasTime" class="font-mono text-xs">{{ row.time?.slice(0, 5) }}</TableCell>
                <TableCell
                  v-for="field in selectedSeries.fields"
                  :key="field"
                  class="text-right font-mono tabular-nums"
                  :class="field === selectedSeries.headline ? 'font-semibold' : ''"
                >{{ formatValue(row[field]) }}</TableCell>
              </TableRow>
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      <!-- A published rate card, which is not anybody's account: no holder, no
           balance, no transactions. A column the sheet left unquoted is blank,
           not zero. -->
      <Card v-if="selectedSheet" class="bg-card border-border shadow-sm">
        <CardHeader class="pb-2">
          <CardTitle class="text-sm font-semibold uppercase tracking-wider text-muted-foreground">
            Rates (₹ per unit) &middot; {{ formatDate(selectedSheet.date) }}
          </CardTitle>
          <CardDescription v-if="selectedSheet.publishedAt">
            Published {{ formatDateTime(selectedSheet.publishedAt) }}
          </CardDescription>
        </CardHeader>
        <CardContent class="overflow-x-auto">
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead class="whitespace-nowrap">Currency</TableHead>
                <TableHead v-for="column in selectedSheet.columns" :key="column" class="whitespace-nowrap text-right">{{ columnLabel(column) }}</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              <TableRow v-for="currency in selectedSheet.currencies" :key="currency.currency">
                <TableCell class="whitespace-nowrap">
                  <span class="font-mono font-semibold">{{ currency.currency }}</span>
                  <span class="ml-2 text-xs text-muted-foreground">{{ currency.name }}</span>
                  <span
                    v-if="rateUnit(currency)"
                    class="ml-2 rounded bg-muted px-1.5 py-0.5 text-[10px] font-bold text-muted-foreground"
                    title="This currency is quoted for this many units, not for one."
                  >{{ rateUnit(currency) }}</span>
                </TableCell>
                <TableCell
                  v-for="column in selectedSheet.columns"
                  :key="column"
                  class="text-right font-mono tabular-nums"
                  :class="currency.rates?.[column] === undefined ? 'text-muted-foreground' : ''"
                >{{ currency.rates?.[column] === undefined ? '—' : currency.rates[column] }}</TableCell>
              </TableRow>
            </TableBody>
          </Table>
        </CardContent>
      </Card>
    </template>
  </div>
</template>
