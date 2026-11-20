// Original runtime-library facts; decode shape only, never infer an execution relationship.
import { arrayOf, boolean, string, byteString, decimalU64, exact, integer, invalid, oneOf, record, decodeChatContent, type Facts } from './wire-json';
import { eventKey } from './event-wire';
import { decodeProcessKey } from './processes-wire';

function signed64(value: unknown) {
  if (typeof value === 'bigint') {
    if (value < -9223372036854775808n || value > 9223372036854775807n) invalid();
  } else if (typeof value !== 'number' || !Number.isSafeInteger(value)) invalid();
}
export function decodeRequestHandoff(value: unknown): Facts {
  const fact = exact(value, ['observation', 'request', 'operation', 'actor', 'observed_boottime_ns', 'phase']);
  eventKey(fact.observation);
  if (fact.request !== null) eventKey(fact.request);
  if (fact.operation !== null) oneOf(fact.operation, ['file_open', 'execute', 'connect', 'send']);
  const actor = exact(fact.actor, ['tgid', 'tid', 'birth_boottime_ns']);
  integer(actor.tgid, 1, 0xffffffff); integer(actor.tid, 1, 0xffffffff); decimalU64(actor.birth_boottime_ns);
  decimalU64(fact.observed_boottime_ns);
  const phase = record(fact.phase);
  switch (phase.type) {
    case 'submission': {
      exact(phase, ['type', 'site', 'call', 'projection', 'analysis', 'projection_error']);
      const site = exact(phase.site, ['process', 'exec_cookie', 'instruction_pointer', 'stack_pointer', 'link_register', 'frame_pointer']);
      decodeProcessKey(site.process);
      for (const key of ['exec_cookie', 'instruction_pointer', 'stack_pointer']) decimalU64(site[key]);
      for (const key of ['link_register', 'frame_pointer']) if (site[key] !== null) decimalU64(site[key]);
      const call = record(phase.call);
      let pointerFields: readonly string[];
      switch (call.type) {
        case 'file_open': {
          exact(call, ['type', 'request_address', 'loop_address', 'path_address', 'path', 'path_status', 'flags', 'mode', 'callback_address']);
          pointerFields = ['request_address', 'loop_address', 'path_address', 'callback_address'];
          byteString(call.path); integer(call.flags, -2147483648, 2147483647); integer(call.mode, -2147483648, 2147483647);
          const status = record(call.path_status);
          if (status.status === 'read_fault') { exact(status, ['status', 'native_error']); integer(status.native_error, -2147483648, -1); }
          else { exact(status, ['status']); oneOf(status.status, ['complete', 'capacity']); }
          break;
        }
        case 'spawn':
          exact(call, ['type', 'loop_address', 'process_address', 'options_address']);
          pointerFields = ['loop_address', 'process_address', 'options_address']; break;
        case 'tcp_connect':
          exact(call, ['type', 'request_address', 'stream_address', 'address', 'callback_address']);
          pointerFields = ['request_address', 'stream_address', 'address', 'callback_address']; break;
        case 'stream_write':
          exact(call, ['type', 'entry', 'request_address', 'stream_address', 'buffers_address', 'buffer_count', 'send_stream_address', 'callback_address']);
          oneOf(call.entry, ['write', 'write2']); integer(call.buffer_count, 0, 0xffffffff);
          if (call.send_stream_address !== null) decimalU64(call.send_stream_address);
          if ((call.entry === 'write') !== (call.send_stream_address === null)) invalid();
          pointerFields = ['request_address', 'stream_address', 'buffers_address', 'callback_address']; break;
        case 'stream_try_write':
          exact(call, ['type', 'entry', 'stream_address', 'buffers_address', 'buffer_count', 'send_stream_address']);
          oneOf(call.entry, ['try_write', 'try_write2']); integer(call.buffer_count, 0, 0xffffffff);
          if (call.send_stream_address !== null) decimalU64(call.send_stream_address);
          if ((call.entry === 'try_write') !== (call.send_stream_address === null)) invalid();
          pointerFields = ['stream_address', 'buffers_address']; break;
        case 'udp_send':
          exact(call, ['type', 'request_address', 'stream_address', 'buffers_address', 'buffer_count', 'address', 'callback_address']);
          integer(call.buffer_count, 0, 0xffffffff);
          pointerFields = ['request_address', 'stream_address', 'buffers_address', 'address', 'callback_address']; break;
        default: return invalid();
      }
      for (const key of pointerFields) decimalU64(call[key]);
      if (phase.projection !== null) byteString(phase.projection);
      if (phase.projection_error !== null) oneOf(phase.projection_error, ['allocation', 'malformed', 'owner_mismatch']);
      if (phase.analysis !== null) {
        const analysis = exact(phase.analysis, ['status', 'candidates']); integer(analysis.status, 0, 3);
        arrayOf(analysis.candidates, value => {
          const candidate = exact(value, ['context', 'proof', 'issues']);
          const context = exact(candidate.context, ['process', 'tool_call_id', 'tool_name', 'response_id', 'run_id', 'session_id', 'session_key', 'agent_id', 'provider', 'api', 'model', 'arguments']);
          decodeProcessKey(context.process); string(context.tool_call_id); string(context.tool_name);
          for (const key of ['response_id', 'run_id', 'session_id', 'session_key', 'agent_id', 'provider', 'api', 'model']) if (context[key] !== null) string(context[key]);
          if (context.arguments !== null) decodeChatContent(context.arguments);
          const proof = exact(candidate.proof, ['frame', 'store', 'tool', 'assistant', 'content', 'content_index', 'reads_consistent']);
          for (const key of ['frame', 'store', 'tool', 'assistant', 'content', 'content_index']) decimalU64(proof[key]);
          boolean(proof.reads_consistent);
          arrayOf(candidate.issues, value => { const issue=exact(value,['object','field','reason']); decimalU64(issue.object);string(issue.field);string(issue.reason); });
        });
      }
      if (phase.projection === null && (phase.analysis !== null || phase.projection_error !== 'allocation')) invalid();
      if (phase.projection !== null && phase.analysis === null && phase.projection_error !== 'malformed') invalid();
      break;
    }
    case 'submit':
      exact(phase, ['type', 'loop_address', 'work_address', 'work_callback', 'done_callback']);
      for (const key of ['loop_address', 'work_address', 'work_callback', 'done_callback']) decimalU64(phase[key]);
      break;
    case 'work_enter': case 'work_leave':
      exact(phase, ['type', 'work_address']); decimalU64(phase.work_address); break;
    case 'done':
      exact(phase, ['type', 'work_address', 'result', 'result_read_error']); decimalU64(phase.work_address);
      if (phase.result !== null) signed64(phase.result);
      if (phase.result_read_error !== null) integer(phase.result_read_error, -2147483648, -1);
      if (phase.result !== null && phase.result_read_error !== null) invalid();
      break;
    case 'cleanup':
      exact(phase, ['type', 'request_address']); decimalU64(phase.request_address); break;
    case 'event_bound':
      exact(phase, ['type', 'event', 'binding', 'enclosing_call']); eventKey(phase.event);
      { const binding=record(phase.binding);
        if(binding.type==='controller_wait'){exact(binding,['type','wait_id']);decimalU64(binding.wait_id);}
        else {exact(binding,['type']);if(binding.type!=='direct_check')invalid();}
      }
      if(phase.enclosing_call!==null)eventKey(phase.enclosing_call);break;
    case 'api_returned': exact(phase,['type','result']);integer(phase.result,-2147483648,2147483647);break;
    case 'child_created': exact(phase,['type','child']);decodeProcessKey(phase.child);break;
    default: invalid();
  }
  return fact;
}
