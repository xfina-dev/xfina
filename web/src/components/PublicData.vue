<script setup>
import { computed, ref, watch } from 'vue';
import { seriesCsv, seriesCoverage } from 'xfina-wasm';
import { ExternalLink, X } from 'lucide-vue-next';

import {
  Card, CardContent, CardDescription, CardHeader, CardTitle,
  Table, TableBody, TableCell, TableHead, TableHeader, TableRow,
} from 'xfina-ui';
import { formatDate, formatDateTime, formatIsoDate, columnLabel, rateUnit } from '@/lib/format.js';
import FileStatus from '@/components/FileStatus.vue';
import StatementHeader from '@/components/StatementHeader.vue';
import { PUBLISHER_GROUPS, groupPublicFormats } from '@/lib/publishers.js';

/**
 * The Market Data tab: what publishers hand to anyone, as opposed to anybody's
 * statement. Price, NAV and index histories, and published rate sheets.
 *
 * Laid out the way Personal Data is: one card per file, under a heading for
 * its group of publishers, and the selected file's details below. A long
 * history arrives in pieces -- a year of NSE index levels per download, five
 * years of AMFI NAVs -- and each piece is a card of its own, next to the other
 * pieces of the same dataset. Nothing here stitches them together; that is
 * computation over the data, and belongs to whoever uses it.
 */
const emit = defineEmits(['remove']);

const props = defineProps({
    // Parsed files whose area is "public".
    entries: { type: Array, required: true },
    // The format list from the library, for the publishers' download pages.
    formats: { type: Array, default: () => [] },
    // A file to bring into view: the last one a drop read.
    focusId: { type: Number, default: null },
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

/**
 * The dataset leads the card, where a statement card leads with its bank:
 * publishers' names run long ("Association of Mutual Funds in India"), and
 * leading with one would truncate the part that tells two cards apart.
 */
const titleOf = (entry) => datasetOf(entry).title;

/** Who published it, and what it holds: the headline, how often, in what. */
const summaryOf = (entry) => {
    const institution = entry.response.institution;
    if (!isSeries(entry)) {
        return `${institution} · ${entry.response.data.currencies?.length || 0} currencies`;
    }
    const d = entry.response.data;
    return [institution, HEADLINE_LABELS[d.headline] || d.headline, d.frequency, d.currency]
        .filter(Boolean)
        .join(' · ');
};

/** The period a file covers, or the day a rate sheet is for. */
const periodOf = (entry) => {
    if (!isSeries(entry)) return formatDate(entry.response.data.date);
    const c = coverageOf(entry);
    return c?.first ? `${formatIsoDate(c.first)} – ${formatIsoDate(c.last)}` : 'No rows';
};

/** Where a file sorts: by dataset, then oldest piece first. */
const sortKey = (entry) =>
    `${datasetOf(entry).key}|${isSeries(entry) ? coverageOf(entry)?.first || '' : sheetDate(entry)}`;

/** The files, as cards under the same publisher groups the empty tab shows. */
const cardGroups = computed(() => {
    const groupOf = (format) =>
        PUBLISHER_GROUPS.find(g => g.ids.includes(format))?.label ?? 'Other';
    const order = [...PUBLISHER_GROUPS.map(g => g.label), 'Other'];
    const groups = new Map(order.map(label => [label, []]));
    for (const entry of props.entries) groups.get(groupOf(entry.response.format)).push(entry);
    return order
        .map(label => ({
            label,
            entries: groups.get(label).sort((a, b) => sortKey(a).localeCompare(sortKey(b))),
        }))
        .filter(g => g.entries.length);
});

/** A rate sheet's date as an ISO string, for ordering it among the others. */
const sheetDate = (entry) => {
    const ts = entry.response?.data?.date;
    return ts ? new Date(Number(ts) * 1000).toISOString().slice(0, 10) : '';
};

const selectedId = ref(null);
const selected = computed(() =>
    props.entries.find(e => e.id === selectedId.value) ?? props.entries.at(-1) ?? null
);
// The last file a drop read is the one shown.
watch(() => props.focusId, (id) => {
    if (id != null) selectedId.value = id;
}, { immediate: true });

const selectedSeries = computed(() => (selected.value && isSeries(selected.value) ? selected.value.response.data : null));
const selectedSheet = computed(() => (selected.value && !isSeries(selected.value) ? selected.value.response.data : null));

/** The newest rows, newest first: what someone checking a download looks at. */
const PREVIEW_ROWS = 15;
const previewRows = computed(() => (selectedSeries.value?.rows || []).slice(-PREVIEW_ROWS).reverse());
const hasTime = computed(() => previewRows.value.some(r => r.time));

/** Value fields a series can carry beside its headline. */
const VALUE_FIELDS = ['close', 'nav', 'totalReturn', 'netTotalReturn'];
/** The one `extra` figure per source worth its latest value in the details. */
const LATEST_EXTRA = { 'md-ishares': 'Shares Outstanding', 'md-spdr-gold': 'Ounces of Gold per Share' };

/**
 * Rows only some sources have, most telling first; the details card shows the
 * first two that apply. A row with nothing to say is left out rather than
 * shown as a dash.
 */
const specificRows = (entry, series, coverage) => {
    const rows = series.rows || [];
    const out = [];
    if (series.frequency === 'intraday') {
        const days = new Set(rows.filter(r => r[series.headline] != null).map(r => r.date)).size;
        out.push({ label: 'Readings / days', value: `${coverage.rows.toLocaleString()} / ${days.toLocaleString()}` });
    }
    const also = VALUE_FIELDS.filter(f => f !== series.headline && series.fields.includes(f));
    if (also.length) out.push({ label: 'Also', value: also.map(f => FIELD_LABELS[f]).join(', ') });
    const dividends = rows.filter(r => r.dividend != null).length;
    const splits = rows.filter(r => r.splitFactor != null).length;
    if (series.fields.includes('dividend') && series.fields.includes('splitFactor')) {
        out.push({ label: 'Dividends / splits', value: `${dividends} / ${splits}` });
    } else if (series.fields.includes('dividend')) {
        out.push({ label: 'Dividends', value: dividends });
    } else if (series.fields.includes('splitFactor')) {
        out.push({ label: 'Splits', value: splits });
    }
    if (series.splitAdjusted != null) {
        out.push({ label: 'Split-adjusted', value: series.splitAdjusted ? 'Yes' : 'No' });
    }
    if (series.declaredRange) {
        const r = series.declaredRange;
        out.push({ label: 'Declared period', value: `${formatDate(r.from)} – ${formatDate(r.to)}` });
    }
    const latest = LATEST_EXTRA[entry.response.format];
    const lastWith = latest && [...rows].reverse().find(r => r.extra?.[latest] != null);
    if (lastWith) out.push({ label: `${latest} (latest)`, value: Number(lastWith.extra[latest]).toLocaleString() });
    if (series.extraFields?.length) out.push({ label: 'Extra fields', value: `${series.extraFields.length} in JSON` });
    return out.slice(0, 2);
};

/**
 * The selected file in Personal Data's three columns: what it is (where a
 * statement shows its holder), what it covers, and its export.
 */
const header = computed(() => {
    const entry = selected.value;
    if (!entry) return null;
    const r = entry.response;
    if (!isSeries(entry)) {
        const sheet = r.data;
        return {
            statementType: 'Reference Rates',
            institutionName: r.institution,
            customerName: 'Forex card rates',
            detailsTitle: 'Sheet Details',
            statementDetails: [
                { label: 'Date', value: formatDate(sheet.date) },
                ...(sheet.publishedAt ? [{ label: 'Published', value: formatDateTime(sheet.publishedAt) }] : []),
                { label: 'Currencies', value: sheet.currencies?.length || 0 },
                { label: 'Rate columns', value: sheet.columns?.length || 0 },
            ],
            csvAvailable: false,
        };
    }
    const d = r.data;
    const c = coverageOf(entry);
    const name = d.name || d.code || entry.name;
    return {
        statementType: 'Market Data',
        institutionName: r.institution,
        // The code is the badge only when it says something the name does not.
        accountNumber: d.code && d.code !== name ? d.code : '',
        customerName: name,
        address: [KIND_LABELS[d.kind], d.currency, d.unit ? `per ${d.unit}` : null].filter(Boolean).join(' · '),
        detailsTitle: 'Series Details',
        statementDetails: [
            { label: 'Type', value: HEADLINE_LABELS[d.headline] || d.headline },
            { label: 'From', value: formatIsoDate(c.first) },
            { label: 'To', value: formatIsoDate(c.last) },
            { label: 'Rows', value: c.rows.toLocaleString() },
            ...specificRows(entry, d, c),
        ],
        csvAvailable: true,
    };
});

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

const publisherGroups = computed(() => groupPublicFormats(props.formats));
</script>

<template>
  <div class="space-y-6">
    <!-- Nothing dropped yet: say what belongs here, and where to get it. -->
    <Card v-if="!entries.length" class="bg-card border-border shadow-sm">
      <CardHeader>
        <CardTitle class="text-base">No market data yet</CardTitle>
        <CardDescription>
          Drop price, NAV or index history downloaded from any of these publishers. Each file is read on its own and shown under the dataset it belongs to.
        </CardDescription>
      </CardHeader>
      <!-- One column per group of publishers, side by side. -->
      <CardContent class="grid gap-6 sm:grid-cols-2 lg:grid-cols-4">
        <div v-for="group in publisherGroups" :key="group.label" class="space-y-2">
          <p class="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">{{ group.label }}</p>
          <div class="flex flex-col items-start gap-2">
            <a
              v-for="format in group.entries"
              :key="format.id"
              :href="format.download_url"
              target="_blank"
              rel="noopener noreferrer"
              class="inline-flex items-center gap-1 rounded-md border border-border px-2 py-1 text-xs hover:bg-muted"
            >
              {{ format.institution }}
              <ExternalLink class="h-3 w-3 text-muted-foreground" />
            </a>
          </div>
        </div>
      </CardContent>
    </Card>

    <template v-else>
      <!-- One card per file, as Personal Data shows statements: the publisher
           and dataset, what it holds, and the period it covers. The cross
           removes that one file. -->
      <div class="space-y-5">
        <div v-for="group in cardGroups" :key="group.label" class="space-y-2">
          <p class="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            {{ group.label }} <span class="font-normal">({{ group.entries.length }})</span>
          </p>
          <div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
            <!-- The card and its cross are sibling buttons: a button inside a
                 button is not valid HTML. -->
            <div v-for="entry in group.entries" :key="entry.id" class="relative">
              <button
                class="flex w-full flex-col gap-0.5 rounded-lg border p-3 pr-9 text-left transition-colors"
                :class="selected?.id === entry.id
                  ? 'border-primary bg-primary/5 ring-1 ring-primary/20'
                  : 'border-border hover:border-primary/40 hover:bg-muted/40'"
                @click="selectedId = entry.id"
              >
                <span class="truncate text-sm font-semibold leading-tight" :title="titleOf(entry)">
                  {{ titleOf(entry) }}
                </span>
                <span class="truncate text-xs leading-tight text-muted-foreground" :title="summaryOf(entry)">
                  {{ summaryOf(entry) }}
                </span>
                <span class="truncate text-xs leading-tight tabular-nums text-muted-foreground" :title="periodOf(entry)">
                  {{ periodOf(entry) }}
                </span>
              </button>
              <button
                class="absolute right-1.5 top-1.5 rounded-md p-1 text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                :aria-label="`Remove ${entry.name}`"
                :title="`Remove ${entry.name}`"
                @click="emit('remove', entry)"
              >
                <X class="h-3.5 w-3.5" />
              </button>
            </div>
          </div>
        </div>

        <!-- Everything below belongs to the one file selected above. -->
        <hr class="border-border" />
      </div>

      <!-- The selected file, laid out as Personal Data lays out a statement:
           the file and its checks, then what it is, what it covers, and its
           export. -->
      <template v-if="selected && header">
        <FileStatus
          :file-info="selected.response.detection.file"
          :validation="selected.response.validation"
          :parse-time="selected.parseTime"
          :no-balances-note="selectedSeries ? 'A price history carries no running balance' : 'A rate sheet carries no running balance'"
        />
        <StatementHeader
          v-bind="header"
          :validation-status="selected.response.validation?.overall"
          export-enabled
          @export-csv="downloadCsv(selected)"
          @export-json="downloadJson(selected)"
        />
      </template>

      <!-- The selected file's newest rows. -->
      <Card v-if="selectedSeries" class="bg-card border-border shadow-sm">
        <CardHeader class="pb-2">
          <CardTitle class="text-sm font-semibold uppercase tracking-wider text-muted-foreground">
            Newest rows
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
