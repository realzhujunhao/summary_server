import os
import re
import textwrap
import datetime
import numpy as np
import srt
import torch
import argparse
import whisper
from openai import OpenAI
import librosa


parser = argparse.ArgumentParser(description="movie to summary model")
parser.add_argument('audio_path', type=str, help='path to input audio in mp3 format')
parser.add_argument('output_dir', type=str, help='generated file dir')
args = parser.parse_args()

mp3_file_path = args.audio_path
output_dir = args.output_dir
input_audio_name = os.path.basename(mp3_file_path)
input_audio_array, sampling_rate = librosa.load(mp3_file_path, sr=16000)

device = torch.device("cuda" if torch.cuda.is_available() else "cpu")
def speech_recognition(model_name, input_audio, output_subtitle_path, decode_options, cache_dir="./"):
    model = whisper.load_model(name=model_name, download_root=cache_dir, device = device)
    transcription = model.transcribe(
        audio=input_audio,
        language=decode_options["language"],
        verbose=False,
        initial_prompt=decode_options["initial_prompt"],
        temperature=decode_options["temperature"]
    )
    subtitles = []
    for i, segment in enumerate(transcription["segments"]):
        start_time = datetime.timedelta(seconds=segment["start"])
        end_time = datetime.timedelta(seconds=segment["end"])
        text = segment["text"]
        subtitles.append(srt.Subtitle(index=i, start=start_time, end=end_time, content=text))
    srt_content = srt.compose(subtitles)
    with open(output_subtitle_path, "w", encoding="utf-8") as file:
        file.write(srt_content)

model_name = 'medium'
language = 'en'
initial_prompt = ''
temperature = 0.0
output_subtitle_path = f"{output_dir}/subtitle.srt"
cache_dir = './'
decode_options = {
    "language": language,
    "initial_prompt": initial_prompt,
    "temperature": temperature
}
speech_recognition(
    model_name=model_name,
    input_audio=input_audio_array,
    output_subtitle_path=output_subtitle_path,
    decode_options=decode_options,
    cache_dir=cache_dir
)
with open(output_subtitle_path, 'r', encoding='utf-8') as file:
    content = file.read()

def extract_and_save_text(srt_filename, output_filename):
    with open(srt_filename, 'r', encoding='utf-8') as file:
        content = file.read()
    pure_text = re.sub(r'\d+\n\d{2}:\d{2}:\d{2},\d{3} --> \d{2}:\d{2}:\d{2},\d{3}\n', '', content)
    pure_text = re.sub(r'\n\n+', '\n', pure_text)
    with open(output_filename, 'w', encoding='utf-8') as output_file:
        output_file.write(pure_text)
    return pure_text

def chunk_text(text, max_length):
    return textwrap.wrap(text, max_length)

chunk_length = 512
convert_to_traditional = False
pure_text = extract_and_save_text(
    srt_filename=output_subtitle_path,
    output_filename=f"{output_dir}/raw_sub.txt",
)
chunks = chunk_text(text=pure_text, max_length=chunk_length)

client = OpenAI()

model_name = 'gpt-4o'
# temperature = 0.0
# top_p = 1.0
# max_tokens = 512

def summarization(client, summarization_prompt, model_name, temperature=0.0, top_p=1.0, max_tokens=512):
    response = client.chat.completions.create(
        messages=[
            {"role": "system", "content": "You are a helpful assistant."},
            {"role": "user", "content": summarization_prompt}
        ],
        model=model_name,
#        temperature=temperature,
#        top_p=top_p,
#        max_tokens=max_tokens
    )
    return response.choices[0].message.content

summarization_prompt_template = "Generate a summary within few sentences：<text>"
paragraph_summaries = []
for index, chunk in enumerate(chunks):
    summarization_prompt = summarization_prompt_template.replace("<text>", chunk)
    summary = summarization(
        client=client,
        summarization_prompt=summarization_prompt,
        model_name=model_name,
        # temperature=temperature,
        # top_p=top_p,
        # max_tokens=max_tokens
    )
    paragraph_summaries.append(summary)
    with open(f"{output_dir}/summary.txt", 'a') as file:
        file.write(summary)


