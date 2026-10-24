<script setup>
import { computed } from 'vue';
import { Card, Tooltip, TooltipContent, TooltipTrigger } from 'xfina-ui';
import { CheckCircle2, AlertTriangle, XCircle, MinusCircle } from 'lucide-vue-next';
import { formatDateTime } from '@/lib/format.js';

/**
 * The file a view is showing: its name, size and age, how its checks came
 * out, and how long it took to read. The same three cards on Personal Data
 * and Market Data, so a file reads the same whichever tab it landed on.
 */
const props = defineProps({
  // `detection.file` from the parse: name, size, modified_timestamp.
  fileInfo: { type: Object, required: true },
  validation: { type: Object, default: null },
  // Seconds, as a string.
  parseTime: { type: [String, Number], default: null },
  // What the Transactions badge says when nothing carried a running balance.
  noBalancesNote: { type: String, default: 'Balances not printed' },
});

// The checks that did not pass, by name and with their note: a count alone
// ("1 / 2 checks passed") leaves the reader guessing which one, and why.
const failedChecks = computed(() => {
  const summary = props.validation?.summary_level;
  return [...(summary?.declared?.checks || []), ...(summary?.derived?.checks || [])]
    .filter(c => !c.passed)
    .map(c => {
      const name = c.name.replaceAll('_', ' ');
      return { name: name[0].toUpperCase() + name.slice(1), note: c.note, computed: c.computed };
    });
});

// A file that prints no totals runs no summary checks, and must not look as
// though it passed them.
const summaryChecksRun = computed(() => {
  const summary = props.validation?.summary_level;
  return (summary?.declared?.checks?.length || 0) + (summary?.derived?.checks?.length || 0) > 0;
});
</script>

<template>
  <div class="grid grid-cols-1 lg:grid-cols-3 gap-4 mb-2">
    <Card class="bg-card text-card-foreground shadow-sm border flex flex-col justify-center p-3 px-4">
      <span class="text-[10px] text-muted-foreground uppercase tracking-wider font-semibold mb-0.5">File Name</span>
      <span class="font-medium text-sm truncate" :title="fileInfo.name">{{ fileInfo.name }}</span>
    </Card>
    
    <Card class="bg-card text-card-foreground shadow-sm border flex items-center justify-between p-3 px-4">
      <div class="flex flex-col">
        <span class="text-[10px] text-muted-foreground uppercase tracking-wider font-semibold mb-0.5">Size</span>
        <span class="font-mono text-sm">{{ (fileInfo.size / 1024).toFixed(1) }} KB</span>
      </div>
      <div class="flex flex-col text-right">
        <span class="text-[10px] text-muted-foreground uppercase tracking-wider font-semibold mb-0.5">Last Modified</span>
        <span class="font-medium text-sm">{{ formatDateTime(fileInfo.modified_timestamp) }}</span>
      </div>
    </Card>
    
    <Card class="bg-card text-card-foreground shadow-sm border flex items-center justify-between p-3 px-4">
      <div class="flex flex-col">
        <span class="text-[10px] text-muted-foreground uppercase tracking-wider font-semibold mb-0.5">Validation Status</span>
        
        <div v-if="validation?.overall" class="flex items-center gap-3">
          <!-- Summary Badge -->
          <Tooltip>
            <TooltipTrigger class="cursor-help flex items-center gap-1.5 font-semibold text-sm w-fit"
                 :class="{
                   'text-muted-foreground': !summaryChecksRun,
                   'text-status-good-text': summaryChecksRun && validation.summary_level?.passed,
                   'text-status-warning-text': summaryChecksRun && !validation.summary_level?.passed && validation.summary_level?.declared?.passed,
                   'text-destructive': summaryChecksRun && !validation.summary_level?.declared?.passed
                 }">
              <!-- Nothing to reconcile against is not a pass. -->
              <MinusCircle v-if="!summaryChecksRun" class="w-4 h-4 opacity-70" />
              <CheckCircle2 v-else-if="validation.summary_level?.passed" class="w-4 h-4" />
              <!-- Only derived checks failed: a warning, not a failure. -->
              <AlertTriangle v-else-if="validation.summary_level?.declared?.passed" class="w-4 h-4" />
              <XCircle v-else class="w-4 h-4" />
              <span>Summary</span>
            </TooltipTrigger>
            <TooltipContent side="bottom" class="p-3 max-w-sm">
              <div class="space-y-3 text-sm">
                <div v-if="validation.summary_level?.declared?.checks?.length > 0">
                  <div class="font-semibold text-foreground mb-0.5">Declared Validations</div>
                  <div class="text-muted-foreground">{{ validation.summary_level.declared.checks.filter(c => c.passed).length }} / {{ validation.summary_level.declared.checks.length }} checks passed</div>
                </div>
                <div v-if="validation.summary_level?.derived?.checks?.length > 0">
                  <div class="font-semibold text-foreground mb-0.5">Derived Validations</div>
                  <div class="text-muted-foreground">{{ validation.summary_level.derived.checks.filter(c => c.passed).length }} / {{ validation.summary_level.derived.checks.length }} checks passed</div>
                </div>
                <div v-if="failedChecks.length">
                  <div class="font-semibold text-foreground mb-0.5">Not passed</div>
                  <ul class="space-y-1 text-muted-foreground">
                    <li v-for="check in failedChecks" :key="check.name">
                      <span class="font-medium text-foreground">{{ check.name }}</span>: {{ check.computed }}<span v-if="check.note"> &middot; {{ check.note }}</span>
                    </li>
                  </ul>
                </div>
                <div v-if="!summaryChecksRun">
                  <div class="font-semibold text-foreground mb-0.5">Statement Summary</div>
                  <div class="text-muted-foreground">Totals not printed (nothing to reconcile)</div>
                </div>
              </div>
            </TooltipContent>
          </Tooltip>

          <!-- Transaction Badge -->
          <Tooltip>
            <TooltipTrigger class="cursor-help flex items-center gap-1.5 font-semibold text-sm w-fit"
                 :class="{
                   'text-muted-foreground': !validation.row_level?.checked_rows,
                   'text-status-good-text': validation.row_level?.checked_rows > 0 && validation.row_level?.passed,
                   'text-destructive': validation.row_level?.checked_rows > 0 && !validation.row_level?.passed
                 }">
              <CheckCircle2 v-if="validation.row_level?.passed && validation.row_level?.checked_rows > 0" class="w-4 h-4" />
              <XCircle v-else-if="!validation.row_level?.passed" class="w-4 h-4" />
              <MinusCircle v-else class="w-4 h-4 opacity-70" />
              <span>Transactions</span>
            </TooltipTrigger>
            <TooltipContent side="bottom" class="p-3 max-w-sm">
              <div class="space-y-1 text-sm">
                <div class="font-semibold text-foreground mb-0.5">Running Transactions</div>
                <div class="text-muted-foreground" v-if="validation.row_level?.checked_rows > 0">
                  {{ (validation.row_level?.checked_rows || 0) - (validation.row_level?.failed_rows?.length || 0) }} / {{ validation.row_level?.checked_rows || 0 }} txns passed
                </div>
                <div class="text-muted-foreground" v-else>
                  {{ noBalancesNote }}
                </div>
              </div>
            </TooltipContent>
          </Tooltip>
        </div>

        <div v-else class="text-sm font-medium text-muted-foreground">-</div>
      </div>
      <div class="flex flex-col text-right">
        <span class="text-[10px] text-muted-foreground uppercase tracking-wider font-semibold mb-0.5">Parse Time</span>
        <span class="font-mono text-sm">{{ parseTime }}s</span>
      </div>
    </Card>
  </div>
</template>
