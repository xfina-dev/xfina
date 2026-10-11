<script setup>
// Xfina runs anonymous usage statistics, so it keeps its own consent dialog.
// The shared header's privacy button opens it (XfinaHeader own-privacy,
// @privacy in App.vue).
import { onMounted, ref } from 'vue';
import { ChevronDown } from 'lucide-vue-next';
import {
  Accordion, AccordionContent, AccordionItem, AccordionTrigger, Badge,
  Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle,
} from 'xfina-ui';
import { getStoredAnalyticsLevel, setStoredAnalyticsLevel, updateAnalyticsState, LEVEL_OFF, LEVEL_ANONYMOUS } from '@/lib/analytics.js';

defineProps({
  // Shown as the example ep.app_version, so the payload is this build's own.
  appVersion: { type: String, required: true },
});
const open = defineModel('open', { type: Boolean, default: false });

const analyticsLevel = ref(getStoredAnalyticsLevel());

const setAnalyticsLevel = (level) => {
    analyticsLevel.value = level;
    setStoredAnalyticsLevel(level);
};

onMounted(() => updateAnalyticsState(analyticsLevel.value));
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="sm:max-w-3xl">
      <DialogHeader>
        <DialogTitle>Privacy &amp; Analytics</DialogTitle>
        <DialogDescription>
          <strong>Help Improve Xfina.</strong> Choose how you'd like to help us make Xfina better.
        </DialogDescription>
      </DialogHeader>
      <div class="space-y-4 py-4">
        <div class="flex items-center space-x-2 p-2 border rounded-md cursor-pointer hover:bg-muted" 
             :class="{'border-primary bg-primary/5': analyticsLevel === LEVEL_ANONYMOUS}"
             @click="setAnalyticsLevel(LEVEL_ANONYMOUS)">
          <div class="flex-1 w-full">
            <Accordion type="single" collapsible class="w-full">
              <AccordionItem value="payload" class="border-b-0">
                
                <div class="flex items-center justify-between w-full">
                  <div class="font-semibold text-base flex items-center">
                    Anonymous Usage Statistics 
                    <Badge class="ml-2">Recommended</Badge>
                  </div>
                  
                  <div @click.stop>
                    <AccordionTrigger class="group hover:no-underline p-0 data-[state=open]:border-b-0">
                      <span class="sr-only">View exact payload details</span>
                      <template #icon>
                        <div class="flex items-center gap-1.5 text-xs font-mono bg-primary/10 text-primary-text pl-2.5 pr-2 py-1.5 rounded shrink-0 ml-2 hover:bg-primary/20 transition-colors">
                          <span>Payload</span>
                          <ChevronDown class="h-4 w-4 transition-transform duration-200 group-data-[state=open]:rotate-180" />
                        </div>
                      </template>
                    </AccordionTrigger>
                  </div>
                </div>

                <div class="text-sm text-muted-foreground mt-2">
                  <p>Help us improve parser quality and performance, and catch broken statement formats by sharing anonymous telemetry. <strong>No personal or financial data is collected.</strong> No file contents, transaction descriptions, financial values, or account numbers are ever included.</p>
                </div>
                
                <div class="mt-3" @click.stop>
                  <AccordionContent>
                      <div class="max-h-[300px] overflow-y-auto pr-2">
                        <div class="rounded-md border overflow-hidden bg-background">
                          <table class="w-full text-left text-sm">
                            <thead class="bg-muted/50 text-muted-foreground">
                              <tr>
                                <th class="px-2 py-1.5 font-medium border-b whitespace-nowrap">Query Param</th>
                                <th class="px-2 py-1.5 font-medium border-b w-full">Description</th>
                                <th class="px-2 py-1.5 font-medium border-b w-1/4">Example Value</th>
                              </tr>
                            </thead>
                            <tbody class="divide-y">
                              <tr>
                                <td class="px-2 py-1.5 font-mono text-primary-text">v</td>
                                <td class="px-2 py-1.5">Standard GA4 protocol version</td>
                                <td class="px-2 py-1.5 font-mono">2</td>
                              </tr>
                              <tr>
                                <td class="px-2 py-1.5 font-mono text-primary-text">tid</td>
                                <td class="px-2 py-1.5">Google Analytics measurement ID</td>
                                <td class="px-2 py-1.5 font-mono">G-WZEYQGS8PE</td>
                              </tr>
                              <tr>
                                <td class="px-2 py-1.5 font-mono text-primary-text">cid</td>
                                <td class="px-2 py-1.5">Randomly generated session ID. Resets on every page load; never stored.</td>
                                <td class="px-2 py-1.5 font-mono">433214986.1786291736</td>
                              </tr>
                              <tr>
                                <td class="px-2 py-1.5 font-mono text-primary-text">en</td>
                                <td class="px-2 py-1.5">Event Name</td>
                                <td class="px-2 py-1.5 font-mono">parser_usage</td>
                              </tr>
                              <tr>
                                <td class="px-2 py-1.5 font-mono text-primary-text">ep.app_version</td>
                                <td class="px-2 py-1.5">Version of the web app and parsers</td>
                                <td class="px-2 py-1.5 font-mono">{{ appVersion }}</td>
                              </tr>
                              <tr>
                                <td class="px-2 py-1.5 font-mono text-primary-text">ep.parser_type</td>
                                <td class="px-2 py-1.5">The type of statement being parsed</td>
                                <td class="px-2 py-1.5 font-mono">icici_ba</td>
                              </tr>
                              <tr>
                                <td class="px-2 py-1.5 font-mono text-primary-text">ep.success</td>
                                <td class="px-2 py-1.5">True if no math/validation errors occurred</td>
                                <td class="px-2 py-1.5 font-mono">false</td>
                              </tr>
                              <tr>
                                <td class="px-2 py-1.5 font-mono text-primary-text">epn.parse_time_ms</td>
                                <td class="px-2 py-1.5">Time taken to process the file locally</td>
                                <td class="px-2 py-1.5 font-mono">2</td>
                              </tr>
                              <tr>
                                <td class="px-2 py-1.5 font-mono text-primary-text">epn.txns_failed</td>
                                <td class="px-2 py-1.5">Total failed transactions <i>(&gt; 0)</i></td>
                                <td class="px-2 py-1.5 font-mono">3</td>
                              </tr>
                              <tr>
                                <td class="px-2 py-1.5 font-mono text-primary-text">epn.declared_failed</td>
                                <td class="px-2 py-1.5">Total failed declared checks <i>(&gt; 0)</i></td>
                                <td class="px-2 py-1.5 font-mono">2</td>
                              </tr>
                              <tr>
                                <td class="px-2 py-1.5 font-mono text-primary-text">epn.derived_failed</td>
                                <td class="px-2 py-1.5">Total failed derived checks <i>(&gt; 0)</i></td>
                                <td class="px-2 py-1.5 font-mono">1</td>
                              </tr>
                            </tbody>
                          </table>
                        </div>
                        <div class="rounded-md border mt-4 overflow-hidden bg-background">
                          <table class="w-full text-left text-sm">
                            <thead class="bg-muted/50 text-muted-foreground">
                              <tr>
                                <th class="px-2 py-1.5 font-medium border-b">Actual Request URL (Example)</th>
                              </tr>
                            </thead>
                            <tbody>
                              <tr>
                                <td class="p-0">
                                  <pre class="p-2 text-xs overflow-x-auto font-mono text-muted-foreground"><code>https://www.google-analytics.com/g/collect?
v=2&amp;
tid=G-WZEYQGS8PE&amp;
cid=433214986.1786291736&amp;
en=parser_usage&amp;
ep.app_version=Unreleased&amp;
ep.parser_type=icici_ba&amp;
ep.success=false&amp;
epn.parse_time_ms=2&amp;
epn.txns_failed=3&amp;
epn.declared_failed=2&amp;
epn.derived_failed=1</code></pre>
                                </td>
                              </tr>
                            </tbody>
                          </table>
                        </div>
                      </div>
                    </AccordionContent>
                  </div>
                </AccordionItem>
              </Accordion>
            </div>
          </div>
        <div class="flex items-center space-x-2 p-2 border rounded-md cursor-pointer hover:bg-muted"
             :class="{'border-primary bg-primary/5': analyticsLevel === LEVEL_OFF}"
             @click="setAnalyticsLevel(LEVEL_OFF)">
          <div class="flex-1">
            <div class="font-semibold text-base">Zero Usage Statistics</div>
            <div class="text-sm text-muted-foreground mt-1">Opt-out completely. We respect your privacy, and no telemetry requests will be sent from your browser. However, without anonymous telemetry, it may take us longer to discover broken statement formats, fix parsing bugs, and improve parser quality and performance.</div>
          </div>
        </div>
      </div>
    </DialogContent>
  </Dialog>
</template>
