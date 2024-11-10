// Imports
import { json, urlencoded } from 'body-parser';
import cors from 'cors';
import express, { Application, NextFunction, Request, Response } from 'express';
import { renameSync, readFileSync } from 'fs';
import multer from 'multer';
import path from 'path';

// Create application
const app: Application = express();

// Application config
app.use(cors());

app.use(json({ limit: '50mb' }));

app.use(urlencoded({ extended: true, limit: '50mb' }));

// Multer config
const storage = multer.diskStorage({
    destination: (_req, _file, cb) => {
        cb(null, 'uploads/');
    },
    filename: (req, file, cb) => {
        cb(null, file.originalname);
    }
});

// Routes
app.post('/files/put', multer({ storage }).single('data'), (req, res) => {
    try {
        const newFileName = req.body.file_name || req.file?.originalname;
        if (!newFileName) {
            throw new Error('File name not provided');
        }
        const newFilePath = path.join('uploads', newFileName);
        renameSync(req.file?.path as string, newFilePath);
        console.log(`File uploaded: ${newFileName}`);
        res.status(200).json({
            data: 'File uploaded',
        });
    } catch (error) {
        console.log(error);
        res.status(500).json({
            data: 'Internal server error'
        });
    }
});

app.get('/files/get/:filename', (req, res) => {
    try {
        const { filename } = req.params;
        const data = readFileSync(`uploads/${filename}`, 'base64');

        res.status(200).json({
            data
        });
    } catch (error) {
        res.status(404).json({
            data: 'File not found'
        });
    }
});

// Health check
app.get('/healthz', (_req, res) => {
    console.log('Health check');
    res.status(200).json({
        data: 'ok'
    });
});

// Catch all
app.use('*', (req, res) => {
    res.status(404).json({
        data: 'Not Found'
    });
});

const PORT = 8007;

app.listen(PORT, () => {
    console.log(`Object storage client is running on port ${PORT}`);
});