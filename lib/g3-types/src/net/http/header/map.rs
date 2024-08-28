/*
 * Copyright 2023 ByteDance and/or its affiliates.
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use http::header::{AsHeaderName, Drain, GetAll};
use http::{HeaderMap, HeaderName};

use super::HttpHeaderValue;

#[derive(Default, Clone)]
pub struct HttpHeaderMap {
    inner: HeaderMap<HttpHeaderValue>,
}

impl HttpHeaderMap {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    #[inline]
    pub fn insert(&mut self, name: HeaderName, value: HttpHeaderValue) -> Option<HttpHeaderValue> {
        self.inner.insert(name, value)
    }

    #[inline]
    pub fn append(&mut self, name: HeaderName, value: HttpHeaderValue) {
        self.inner.append(name, value);
    }

    #[inline]
    pub fn remove<K: AsHeaderName>(&mut self, name: K) -> Option<HttpHeaderValue> {
        self.inner.remove(name)
    }

    #[inline]
    pub fn contains_key<K: AsHeaderName>(&self, name: K) -> bool {
        self.inner.contains_key(name)
    }

    #[inline]
    pub fn get<K: AsHeaderName>(&self, name: K) -> Option<&HttpHeaderValue> {
        self.inner.get(name)
    }

    #[inline]
    pub fn get_all<K: AsHeaderName>(&self, name: K) -> GetAll<'_, HttpHeaderValue> {
        self.inner.get_all(name)
    }

    pub fn for_each<F>(&self, mut call: F)
    where
        F: FnMut(&HeaderName, &HttpHeaderValue),
    {
        self.inner
            .iter()
            .for_each(|(name, value)| call(name, value));
    }

    pub fn drain(&mut self) -> Drain<'_, HttpHeaderValue> {
        self.inner.drain()
    }
}

impl From<HttpHeaderMap> for HeaderMap {
    fn from(mut value: HttpHeaderMap) -> Self {
        let mut new_map = HeaderMap::with_capacity(value.inner.capacity());

        let mut last_name: Option<HeaderName> = None;
        for (name, value) in value.inner.drain() {
            match name {
                Some(name) => {
                    last_name = Some(name.clone());
                    new_map.append(name, value.into());
                }
                None => {
                    let Some(name) = &last_name else {
                        break;
                    };
                    new_map.append(name, value.into());
                }
            }
        }
        new_map
    }
}

impl From<&HttpHeaderMap> for HeaderMap {
    fn from(value: &HttpHeaderMap) -> Self {
        let mut new_map = HeaderMap::with_capacity(value.inner.capacity());
        value.for_each(|name, value| {
            new_map.append(name, value.into());
        });
        new_map
    }
}
