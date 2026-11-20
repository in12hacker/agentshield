/// <reference types="node" />
import { describe, it, expect, vi } from 'vitest';
import { readFileSync } from 'node:fs';
import { renderToStaticMarkup } from 'react-dom/server';
import { decodeChatData, matchesChatPage, chatCommand } from './chat-wire';
import { ChatHistoryMessage } from './chat-message';
import { I18nProvider } from './i18n';
import { parseUnique, wireJson } from './wire-json';
import { gatewayReference } from './gateway-wire';
import { ChatHistory, emptyChat } from './chat-state';

const message = { id: '1', capture: Array(32).fill(1), from_client: false, frames: [{ offset: '0', length: '10' }] };
const field = { message, pointer: '/payload/message' };
const process = { boot_id: Array(16).fill(1), host_tgid: 20, birth_boottime_ns: '42' };
const source = { capture: message.capture, source: Array(16).fill(2), generation: '1', first_syn_ordinal: '1', client_port: 40001, server_port: 18789,
  client: process, server: process, client_socket_cookie: '10', server_socket_cookie: '20' };
function nativePage() { return {
  type: 'chat_messages', next_after: null, messages: [{ kind: 'native_gateway', reference: { kind: 'native_gateway', object: field }, native_message_id: {kind:'json_string',value:'original'},
    order: { kind: 'unresolved', references: [field] }, gaps: [{ reason:'message_order_unproven',references:[field] }], observations: [{
      source, reference: field, wrapper: null, history: null, session: null, handoff: null, run_id: '', message_run_id: null,
      message_sequence: null, wrapper_sequence: null, record_timestamp_ms: 0, transcript_position: null, native_fields: [],
      role: { kind:'string',value:{kind:'tool'},original:'toolResult',field:{message,pointer:'/payload/message/role'} },
      content: { kind:'json',value:parseUnique('["",null,0,false,9007199254740993,1.2300e+400]') }, state:false,revision:null,
    }] }],
}; }
describe('source-specific native chat consumers', () => {
  it('shows equal message content once while retaining distinct source observations; differing source content stays separate', () => {
    const input = nativePage();
    const original = input.messages[0]!.observations[0]!;
    original.content = { kind: 'text', value: 'same observed body' } as typeof original.content;
    const repeated = { ...original, reference: { ...original.reference, pointer: '/payload/messages/1' } };
    input.messages[0]!.observations.push(repeated);
    let data = decodeChatData(input);
    if (data.type !== 'chat_messages') throw Error('page');
    let markup = renderToStaticMarkup(<I18nProvider><ChatHistoryMessage entry={data.messages[0]!} /></I18nProvider>);
    expect(markup.match(/class="chat-body"/g)).toHaveLength(1);
    expect(data.messages[0]!.kind === 'native_gateway' && data.messages[0]!.observations).toHaveLength(2);
    repeated.content = { kind: 'text', value: 'another observed body' } as typeof original.content;
    data = decodeChatData(input);
    if (data.type !== 'chat_messages') throw Error('page');
    markup = renderToStaticMarkup(<I18nProvider><ChatHistoryMessage entry={data.messages[0]!} /></I18nProvider>);
    expect(markup.match(/class="chat-body"/g)).toHaveLength(2);
    expect(markup).toContain('same observed body');
    expect(markup).toContain('another observed body');
  });

  it('preserves native null, empty and numeric values with the shared content renderer in all languages', () => {
    const input = nativePage(), data = decodeChatData(input); expect(data).toBe(input);
    expect(matchesChatPage(data,{type:'chat_messages',scope:{kind:'native_gateway',session:null},after:null,limit:8})).toBe(true);
    if (data.type !== 'chat_messages') throw Error('page');
    for (const locale of ['ja-JP','zh-CN','en-US']) {
      vi.stubGlobal('localStorage',{getItem:()=>locale});
      try { const markup=renderToStaticMarkup(<I18nProvider><ChatHistoryMessage entry={data.messages[0]!}/></I18nProvider>);
        expect(markup).toContain('9007199254740993');expect(markup).toContain('1.2300e+400');expect(wireJson(data)).toContain('toolResult');
        expect(markup).toContain('null');expect(markup).toContain('false');
      } finally {vi.unstubAllGlobals();}
    }
    expect(()=>decodeChatData({type:'chat_messages',messages:[{id:{}}],next_after:null})).toThrow();
    expect(()=>chatCommand(Array(16).fill(1),{type:'chat_messages',scope:{kind:'native_gateway',session:null},after:{kind:'model_http',message:{exchange:{flow_instance:Array(16).fill(1),sequence:'0'},sequence:'0'}},limit:8})).toThrow();
    expect(()=>gatewayReference({...message,frames:[{offset:'10',length:'10'},{offset:'19',length:'1'}]})).toThrow();
    expect(wireJson(data)).toContain('1.2300e+400');
  });
  it.skipIf(!globalThis.process.env.AS_SESSION_SOFTWARE_REPLY)('decodes and renders the backend candidate reply from authorized ordinary-client software supply', () => {
    const rows=parseUnique(readFileSync(globalThis.process.env.AS_SESSION_SOFTWARE_REPLY!,'utf8')) as {session:unknown;page:unknown}[];
    expect(rows.length).toBeGreaterThan(0);
    for (const row of rows) {
      const directory=decodeChatData({type:'chat_sessions',sessions:[row.session],next_before:null});
      const data=decodeChatData(row.page);
      if (directory.type!=='chat_sessions'||data.type!=='chat_messages') throw Error('software page');
      expect(matchesChatPage(data,{type:'chat_messages',scope:{kind:'native_gateway',session:directory.sessions[0]!.reference},after:null,limit:64})).toBe(true);
      let state=emptyChat(true);const commands:string[]=[];
      const history=new ChatHistory({read:()=>state,publish:next=>{state=next;},granted:()=>true,command:(encode,begin)=>{
        const id=Array(16).fill(commands.length+1);begin(id);commands.push(encode(id));
      }});
      state={...state,sessions:{...state.sessions,phase:'returned',result:{status:'completed',data:directory}}};
      const selected=directory.sessions[0]!;
      history.selectSession(selected);history.close();history.selectSession(selected);
      expect(commands).toHaveLength(2);
      for(const command of commands) expect(JSON.parse(command).business.query).toMatchObject({type:'chat_messages',scope:{kind:'native_gateway',session:selected.reference},after:null});
      for (const entry of data.messages) expect(renderToStaticMarkup(<I18nProvider><ChatHistoryMessage entry={entry}/></I18nProvider>)).toContain('chat-message');
    }
  });
});
