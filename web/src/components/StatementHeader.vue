<script setup>
import { Button, Card, CardContent, CardHeader, CardTitle, CardDescription } from 'xfina-ui';
import { Download, FileJson, CheckCircle2, AlertTriangle, XCircle } from 'lucide-vue-next';

const props = defineProps({
  // Customer Card
  customerName: {
    type: String,
    required: true
  },
  institutionName: {
    type: String,
    default: ''
  },
  statementType: {
    type: String,
    default: ''
  },
  accountNumber: {
    type: String,
    default: ''
  },
  address: {
    type: String,
    default: ''
  },
  customerId: {
    type: String,
    default: ''
  },
  
  // Statement Card (Array of { label, value })
  statementDetails: {
    type: Array,
    default: () => []
  },
  validationStatus: {
    type: String,
    default: null
  }
});
</script>

<template>
  <div class="grid grid-cols-1 md:grid-cols-3 gap-4 mb-6">
    <!-- Customer/Investor Card -->
    <Card class="bg-card text-card-foreground shadow-sm flex flex-col h-full">
      <CardHeader class="pb-1">
        <div class="text-xs text-muted-foreground font-semibold uppercase tracking-wider flex items-center gap-2">
          <span v-if="statementType">{{ statementType }}</span>
          <span v-if="institutionName && statementType" class="text-muted-foreground/50">|</span>
          <span v-if="institutionName">{{ institutionName }}</span>
        </div>


      </CardHeader>
      <CardContent class="mt-1 flex-1">
        <div class="flex flex-col gap-1">
          <div class="flex items-center gap-2 mb-1 flex-wrap">
            <span class="text-sm font-medium font-mono bg-muted/30 border border-primary/20 rounded px-2.5 py-1 text-primary-text shadow-sm" v-if="accountNumber">{{ accountNumber }}</span>
            <span class="text-sm font-medium font-mono bg-muted/30 border border-primary/20 rounded px-2.5 py-1 text-primary-text shadow-sm" v-if="customerId">CUST ID: {{ customerId }}</span>
          </div>
          <span class="text-xl font-bold">{{ customerName }}</span>
          <span class="text-xs text-muted-foreground leading-snug" v-if="address">{{ address }}</span>
        </div>
      </CardContent>
    </Card>

    <!-- Statement Card -->
    <Card class="bg-card text-card-foreground shadow-sm flex flex-col h-full">
      <CardHeader class="pb-2">
        <CardTitle class="text-sm text-muted-foreground font-semibold uppercase tracking-wider">Statement Details</CardTitle>
      </CardHeader>
      <CardContent>
        <table class="w-full text-sm">
          <tbody>
            <tr v-for="(detail, idx) in statementDetails" :key="idx" class="border-0">
              <td class="py-1 text-muted-foreground font-medium align-top pr-4">{{ detail.label }}</td>
              <td class="py-1 text-foreground font-medium whitespace-nowrap text-right sm:text-left">
                {{ detail.value }}
                <span v-if="detail.derived" title="Estimated date" class="ml-1 text-[10px] font-semibold text-muted-foreground/70 bg-muted/40 rounded px-1 py-0.5 align-middle">est.</span>
              </td>
            </tr>
          </tbody>
        </table>
      </CardContent>
    </Card>

    <!-- Export Card -->
    <Card class="bg-card text-card-foreground shadow-sm flex flex-col h-full border-dashed border-muted-foreground/30">
      <CardHeader class="pb-2">
        <CardTitle class="text-sm text-muted-foreground font-semibold uppercase tracking-wider">Export Data</CardTitle>
      </CardHeader>
      <CardContent class="flex flex-col gap-3 justify-center flex-1">
        <Button variant="outline" class="w-full justify-start gap-2" disabled title="Export to CSV (Coming Soon)">
          <Download class="h-4 w-4" />
          Export CSV
        </Button>
        <Button variant="outline" class="w-full justify-start gap-2" disabled title="Export to JSON (Coming Soon)">
          <FileJson class="h-4 w-4" />
          Export JSON
        </Button>
      </CardContent>
    </Card>
  </div>
</template>
