#![allow(unused, non_upper_case_globals)]

use cocoa::appkit::CGFloat;
use core_foundation::{
    array::{
        CFArray, CFArrayAppendArray, CFArrayAppendValue, CFArrayCreateMutable, CFArrayGetCount,
        CFArrayGetValueAtIndex, CFArrayRef, CFMutableArrayRef, kCFTypeArrayCallBacks,
    },
    base::{CFRelease, TCFType, kCFAllocatorDefault},
    dictionary::{
        CFDictionaryCreate, kCFTypeDictionaryKeyCallBacks, kCFTypeDictionaryValueCallBacks,
    },
    number::{CFNumber, CFNumberRef},
    string::{CFString, CFStringRef},
};
use core_foundation_sys::locale::CFLocaleCopyPreferredLanguages;
use core_graphics::{display::CFDictionary, geometry::CGAffineTransform};
use core_text::font_descriptor::{
    TraitAccessors, kCTFontFamilyNameAttribute, kCTFontItalicTrait, kCTFontSlantTrait,
    kCTFontTraitsAttribute, kCTFontWeightTrait, kCTFontWidthTrait,
};
use core_text::{
    font::{CTFont, CTFontRef, cascade_list_for_languages},
    font_descriptor::{
        CTFontDescriptor, CTFontDescriptorCopyAttributes, CTFontDescriptorCreateCopyWithFeature,
        CTFontDescriptorCreateWithAttributes, CTFontDescriptorCreateWithNameAndSize,
        CTFontDescriptorRef, kCTFontCascadeListAttribute, kCTFontFeatureSettingsAttribute,
    },
};
use font_kit::font::Font as FontKitFont;
use gpui::{FontFallbacks, FontFeatures, FontWeight};
use std::ptr;

pub fn apply_features_and_fallbacks(
    font: &mut FontKitFont,
    features: &FontFeatures,
    fallbacks: Option<&FontFallbacks>,
    weight: FontWeight,
) -> anyhow::Result<()> {
    unsafe {
        let mut keys = vec![kCTFontFeatureSettingsAttribute];
        let mut values = vec![generate_feature_array(features)];
        if let Some(fallbacks) = fallbacks
            && !fallbacks.fallback_list().is_empty()
        {
            keys.push(kCTFontCascadeListAttribute);
            values.push(generate_fallback_array(fallbacks, font, weight));
        }
        let attrs = CFDictionaryCreate(
            kCFAllocatorDefault,
            keys.as_ptr() as _,
            values.as_ptr() as _,
            keys.len() as isize,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        );

        for value in &values {
            CFRelease(*value as _);
        }

        let new_descriptor = CTFontDescriptorCreateWithAttributes(attrs);
        CFRelease(attrs as _);
        let new_descriptor = CTFontDescriptor::wrap_under_create_rule(new_descriptor);
        let new_font = CTFontCreateCopyWithAttributes(
            font.native_font().as_concrete_TypeRef(),
            0.0,
            std::ptr::null(),
            new_descriptor.as_concrete_TypeRef(),
        );
        let new_font = CTFont::wrap_under_create_rule(new_font);
        *font = font_kit::font::Font::from_native_font(&new_font);

        Ok(())
    }
}

fn generate_feature_array(features: &FontFeatures) -> CFMutableArrayRef {
    unsafe {
        let feature_array = CFArrayCreateMutable(kCFAllocatorDefault, 0, &kCFTypeArrayCallBacks);
        for (tag, value) in features.tag_value_list() {
            let keys = [kCTFontOpenTypeFeatureTag, kCTFontOpenTypeFeatureValue];
            let values = [
                CFString::new(tag).as_CFTypeRef(),
                CFNumber::from(*value as i32).as_CFTypeRef(),
            ];
            let dict = CFDictionaryCreate(
                kCFAllocatorDefault,
                &keys as *const _ as _,
                &values as *const _ as _,
                2,
                &kCFTypeDictionaryKeyCallBacks,
                &kCFTypeDictionaryValueCallBacks,
            );
            values.into_iter().for_each(|value| CFRelease(value));
            CFArrayAppendValue(feature_array, dict as _);
            CFRelease(dict as _);
        }
        feature_array
    }
}

fn generate_fallback_array(
    fallbacks: &FontFallbacks,
    font: &mut FontKitFont,
    weight: FontWeight,
) -> CFMutableArrayRef {
    unsafe {
        let symbolic_traits = font.native_font().symbolic_traits();
        let fallback_array = CFArrayCreateMutable(kCFAllocatorDefault, 0, &kCFTypeArrayCallBacks);
        for user_fallback in fallbacks.fallback_list() {
            let name = CFString::from(user_fallback.as_str());

            let traits_keys = [kCTFontWeightTrait, kCTFontSlantTrait];
            let weight_value = CFNumber::from(core_text_weight_trait(weight));
            let slant_value = CFNumber::from(if (symbolic_traits & kCTFontItalicTrait) != 0 {
                1.0
            } else {
                0.0
            });
            let traits_values = [weight_value.as_CFTypeRef(), slant_value.as_CFTypeRef()];
            let traits = CFDictionaryCreate(
                kCFAllocatorDefault,
                &traits_keys as *const _ as _,
                &traits_values as *const _ as _,
                traits_keys.len() as isize,
                &kCFTypeDictionaryKeyCallBacks,
                &kCFTypeDictionaryValueCallBacks,
            );
            drop(weight_value);
            drop(slant_value);

            let attr_keys = [kCTFontFamilyNameAttribute, kCTFontTraitsAttribute];
            let attr_values = [name.as_CFTypeRef(), traits as _];
            let attrs = CFDictionaryCreate(
                kCFAllocatorDefault,
                &attr_keys as *const _ as _,
                &attr_values as *const _ as _,
                attr_keys.len() as isize,
                &kCFTypeDictionaryKeyCallBacks,
                &kCFTypeDictionaryValueCallBacks,
            );
            CFRelease(traits as _);

            let fallback_desc = CTFontDescriptorCreateWithAttributes(attrs);
            CFRelease(attrs as _);
            let fallback_desc = CTFontDescriptor::wrap_under_create_rule(fallback_desc);
            let fallback_desc = descriptor_with_weight(&fallback_desc, weight);

            CFArrayAppendValue(fallback_array, fallback_desc.as_CFTypeRef());
        }

        let font_ref = font.native_font().as_concrete_TypeRef();
        append_system_fallbacks(fallback_array, font_ref, weight);
        fallback_array
    }
}

fn append_system_fallbacks(
    fallback_array: CFMutableArrayRef,
    font_ref: CTFontRef,
    weight: FontWeight,
) {
    unsafe {
        let preferred_languages: CFArray<CFString> =
            CFArray::wrap_under_create_rule(CFLocaleCopyPreferredLanguages());

        let default_fallbacks = CTFontCopyDefaultCascadeListForLanguages(
            font_ref,
            preferred_languages.as_concrete_TypeRef(),
        );
        let default_fallbacks: CFArray<CTFontDescriptor> =
            CFArray::wrap_under_create_rule(default_fallbacks);

        for desc in default_fallbacks
            .iter()
            .filter(|desc| desc.font_path().is_some())
        {
            let desc = descriptor_with_weight(&desc, weight);
            CFArrayAppendValue(fallback_array, desc.as_concrete_TypeRef() as _);
        }
    }
}

const WEIGHT_AXIS_IDENTIFIER: i64 = u32::from_be_bytes(*b"wght") as i64;
const CORE_TEXT_WEIGHT_MAPPING: [f32; 9] = [-0.7, -0.5, -0.23, 0.0, 0.2, 0.3, 0.4, 0.6, 0.8];

fn core_text_weight_trait(weight: FontWeight) -> f32 {
    let position = (weight.0.clamp(FontWeight::THIN.0, FontWeight::BLACK.0) - 100.0) / 100.0;
    let lower = position.floor() as usize;
    let upper = position.ceil() as usize;
    let fraction = position - lower as f32;
    CORE_TEXT_WEIGHT_MAPPING[lower]
        + (CORE_TEXT_WEIGHT_MAPPING[upper] - CORE_TEXT_WEIGHT_MAPPING[lower]) * fraction
}

fn descriptor_with_weight(descriptor: &CTFontDescriptor, weight: FontWeight) -> CTFontDescriptor {
    let identifier = CFNumber::from(WEIGHT_AXIS_IDENTIFIER);
    let varied = unsafe {
        CTFontDescriptorCreateCopyWithVariation(
            descriptor.as_concrete_TypeRef(),
            identifier.as_concrete_TypeRef(),
            weight.0 as CGFloat,
        )
    };
    if varied.is_null() {
        unsafe { CTFontDescriptor::wrap_under_get_rule(descriptor.as_concrete_TypeRef()) }
    } else {
        unsafe { CTFontDescriptor::wrap_under_create_rule(varied) }
    }
}

#[link(name = "CoreText", kind = "framework")]
unsafe extern "C" {
    static kCTFontOpenTypeFeatureTag: CFStringRef;
    static kCTFontOpenTypeFeatureValue: CFStringRef;

    fn CTFontDescriptorCreateCopyWithVariation(
        original: CTFontDescriptorRef,
        variation_identifier: CFNumberRef,
        variation_value: CGFloat,
    ) -> CTFontDescriptorRef;

    fn CTFontCreateCopyWithAttributes(
        font: CTFontRef,
        size: CGFloat,
        matrix: *const CGAffineTransform,
        attributes: CTFontDescriptorRef,
    ) -> CTFontRef;
    fn CTFontCopyDefaultCascadeListForLanguages(
        font: CTFontRef,
        languagePrefList: CFArrayRef,
    ) -> CFArrayRef;
}
