import { expect, it } from 'vitest';
import { attributionInput, secondsToNanoseconds } from './attribution-input';

it('preserves exact explicit seconds down to nanoseconds and the unsigned boundary', () => {
  expect(secondsToNanoseconds('0')).toBe('0');
  expect(secondsToNanoseconds('0.000000001')).toBe('1');
  expect(secondsToNanoseconds('18446744073.709551615')).toBe('18446744073709551615');
  for (const value of ['', '1e3', '-1', '0.0000000001', '18446744073.709551616', '1\n'])
    expect(() => secondsToNanoseconds(value)).toThrow();
});
it('uses the whole value only when explicitly chosen and preserves a literal zero argv index', () => {
  const f=new FormData();f.set('actor','same_thread');f.set('lookback_seconds','0');f.set('target','exec_argument');
  f.set('argument_index','0');f.set('limit','8');
  expect(() => attributionInput(f)).toThrow();
  f.set('value_pointer_mode','whole');
  expect(attributionInput(f)).toEqual({pattern:{actor:'same_thread',lookback_ns:'0',target:{type:'exec_argument',pointer:'',index:0}},after:null,limit:8});
  f.set('value_pointer_mode','pointer');f.set('value_pointer','/~0x/~1');
  expect(attributionInput(f).pattern.target).toMatchObject({pointer:'/~0x/~1'});
  f.set('value_pointer','/~2');expect(() => attributionInput(f)).toThrow();
});
