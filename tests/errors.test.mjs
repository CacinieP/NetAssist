import assert from 'node:assert/strict';
import test from 'node:test';
import { invokeErrorMessage } from '../src/utils/errors.ts';

test('string rejections from Tauri commands keep the backend message', () => {
  assert.equal(invokeErrorMessage('用户取消了管理员授权', '修复操作失败'), '用户取消了管理员授权');
  assert.equal(invokeErrorMessage('Adapter reset is not supported on this platform', 'x'), 'Adapter reset is not supported on this platform');
});

test('Error objects use their message', () => {
  assert.equal(invokeErrorMessage(new Error('boom'), '修复操作失败'), 'boom');
});

test('empty or missing errors fall back', () => {
  for (const value of ['', '   ', null, undefined, new Error('')]) {
    assert.equal(invokeErrorMessage(value, '修复操作失败'), '修复操作失败');
  }
});
